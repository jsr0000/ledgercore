//! Integration tests over the real tonic stack: server bound to an
//! ephemeral TCP port, generated client connecting to it. Each test
//! gets a fresh DB via #[sqlx::test].

use std::sync::Arc;

use app::LedgerRepo;
use domain::{Account, AccountId, AccountKind, Currency};
use infra_pg::PgLedgerRepo;
use ledger_svc::adapters::{SystemClock, UuidV4Gen};
use ledger_svc::proto::ledger_client::LedgerClient;
use ledger_svc::proto::ledger_server::LedgerServer;
use ledger_svc::proto::{
    self, GetAccountHistoryRequest, GetBalanceRequest, PostTransactionRequest,
};
use ledger_svc::LedgerService;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use std::str::FromStr;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};
use tonic::Code;
use uuid::Uuid;

// --- harness -----------------------------------------------------------------

async fn spin_up(pool: PgPool) -> (tokio::task::JoinHandle<()>, LedgerClient<Channel>) {
    let repo: Arc<dyn LedgerRepo> = Arc::new(PgLedgerRepo::new(pool));
    let svc = LedgerService::new(repo, Arc::new(SystemClock), Arc::new(UuidV4Gen));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);

    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(LedgerServer::new(svc))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    let client = LedgerClient::connect(format!("http://{addr}"))
        .await
        .expect("client connect");
    (server, client)
}

fn account(id: u128, kind: AccountKind, allow_negative: bool) -> Account {
    Account::new(
        AccountId::new(Uuid::from_u128(id)),
        kind,
        Currency::Usd,
        allow_negative,
    )
}

fn wire_entry(account_id: AccountId, direction: proto::Direction, amount: &str) -> proto::Entry {
    proto::Entry {
        account_id: account_id.as_uuid().to_string(),
        direction: direction as i32,
        amount: Some(proto::Money {
            amount: amount.into(),
            currency: proto::Currency::Usd as i32,
        }),
    }
}

async fn seed_accounts(pool: &PgPool) -> (Account, Account) {
    let cash = account(1, AccountKind::Asset, false);
    let customer = account(2, AccountKind::Liability, false);
    let repo = PgLedgerRepo::new(pool.clone());
    repo.create_account(cash.clone()).await.unwrap();
    repo.create_account(customer.clone()).await.unwrap();
    (cash, customer)
}

// --- happy path --------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn post_transaction_then_balance_round_trip(pool: PgPool) {
    let (cash, customer) = seed_accounts(&pool).await;
    let (_server, mut client) = spin_up(pool).await;

    let resp = client
        .post_transaction(PostTransactionRequest {
            idempotency_key: "deposit-1".into(),
            entries: vec![
                wire_entry(cash.id(), proto::Direction::Debit, "100.00"),
                wire_entry(customer.id(), proto::Direction::Credit, "100.00"),
            ],
            occurred_at: None,
        })
        .await
        .unwrap();
    assert!(!resp.into_inner().transaction_id.is_empty());

    let bal = client
        .get_balance(GetBalanceRequest {
            account_id: cash.id().as_uuid().to_string(),
        })
        .await
        .unwrap();
    // Compare by value, not by string: NUMERIC(38,9) returns the column's
    // scale on read (e.g. "100.000000000") regardless of input scale.
    let money = bal.into_inner().balance.unwrap();
    assert_eq!(Decimal::from_str(&money.amount).unwrap(), dec!(100));
    assert_eq!(money.currency, proto::Currency::Usd as i32);

    let history = client
        .get_account_history(GetAccountHistoryRequest {
            account_id: cash.id().as_uuid().to_string(),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(history.into_inner().entries.len(), 1);
}

// --- INV6 --------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_idempotency_key_returns_same_transaction_id(pool: PgPool) {
    let (cash, customer) = seed_accounts(&pool).await;
    let (_server, mut client) = spin_up(pool).await;

    let mk = || PostTransactionRequest {
        idempotency_key: "dedupe-me".into(),
        entries: vec![
            wire_entry(cash.id(), proto::Direction::Debit, "50"),
            wire_entry(customer.id(), proto::Direction::Credit, "50"),
        ],
        occurred_at: None,
    };

    let first = client.post_transaction(mk()).await.unwrap().into_inner().transaction_id;
    let second = client.post_transaction(mk()).await.unwrap().into_inner().transaction_id;

    assert_eq!(first, second, "replay returns the original id");

    let bal = client
        .get_balance(GetBalanceRequest {
            account_id: cash.id().as_uuid().to_string(),
        })
        .await
        .unwrap()
        .into_inner()
        .balance
        .unwrap();
    assert_eq!(
        Decimal::from_str(&bal.amount).unwrap(),
        dec!(50),
        "no double posting"
    );
}

// --- INV5 --------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn non_negative_violation_is_failed_precondition(pool: PgPool) {
    let (cash, customer) = seed_accounts(&pool).await;
    let (_server, mut client) = spin_up(pool).await;

    // Asset credit decreases balance; from 0 → -100 violates allow_negative=false.
    let err = client
        .post_transaction(PostTransactionRequest {
            idempotency_key: "would-go-neg".into(),
            entries: vec![
                wire_entry(cash.id(), proto::Direction::Credit, "100"),
                wire_entry(customer.id(), proto::Direction::Debit, "100"),
            ],
            occurred_at: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
}

// --- malformed inputs --------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn malformed_account_id_is_invalid_argument(pool: PgPool) {
    let (_server, mut client) = spin_up(pool).await;

    let err = client
        .get_balance(GetBalanceRequest {
            account_id: "not-a-uuid".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unspecified_direction_is_invalid_argument(pool: PgPool) {
    let (cash, customer) = seed_accounts(&pool).await;
    let (_server, mut client) = spin_up(pool).await;

    let err = client
        .post_transaction(PostTransactionRequest {
            idempotency_key: "bad-dir".into(),
            entries: vec![
                proto::Entry {
                    account_id: cash.id().as_uuid().to_string(),
                    direction: proto::Direction::Unspecified as i32,
                    amount: Some(proto::Money {
                        amount: "1".into(),
                        currency: proto::Currency::Usd as i32,
                    }),
                },
                wire_entry(customer.id(), proto::Direction::Credit, "1"),
            ],
            occurred_at: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
}

// --- NotFound ----------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn missing_account_balance_is_not_found(pool: PgPool) {
    let (_server, mut client) = spin_up(pool).await;

    let err = client
        .get_balance(GetBalanceRequest {
            account_id: Uuid::from_u128(99).to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
}
