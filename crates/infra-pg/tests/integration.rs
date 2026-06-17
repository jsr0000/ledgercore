//! Integration tests for the Postgres adapter. Each test gets a fresh
//! database via #[sqlx::test], with migrations applied from the
//! workspace-root `migrations/` directory.

use std::sync::Arc;

use app::{LedgerError, LedgerRepo};
use domain::{
    Account, AccountId, AccountKind, Currency, Direction, Entry, IdempotencyKey, Money,
    Transaction, TransactionId,
};
use infra_pg::PgLedgerRepo;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use time::macros::datetime;
use time::OffsetDateTime;
use uuid::Uuid;

// --- helpers -----------------------------------------------------------------

fn account(id: u128, kind: AccountKind, allow_negative: bool) -> Account {
    Account::new(
        AccountId::new(Uuid::from_u128(id)),
        kind,
        Currency::Usd,
        allow_negative,
    )
}

fn entry(account: AccountId, direction: Direction, amount: Decimal) -> Entry {
    Entry::new(account, direction, Money::new(amount, Currency::Usd)).unwrap()
}

fn at() -> OffsetDateTime {
    datetime!(2026-06-17 12:00:00 UTC)
}

fn tx(id: u128, key: &str, entries: Vec<Entry>) -> Transaction {
    Transaction::new(
        TransactionId::new(Uuid::from_u128(id)),
        IdempotencyKey::new(key).unwrap(),
        entries,
        at(),
    )
    .unwrap()
}

// --- happy path --------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn happy_path_updates_both_balances(pool: PgPool) {
    let repo = PgLedgerRepo::new(pool);
    let cash = account(1, AccountKind::Asset, false);
    let customer = account(2, AccountKind::Liability, false);
    repo.create_account(cash.clone()).await.unwrap();
    repo.create_account(customer.clone()).await.unwrap();

    // Customer deposits 100: cash debit, customer-liability credit.
    repo.post_transaction(tx(
        100,
        "deposit-1",
        vec![
            entry(cash.id(), Direction::Debit, dec!(100)),
            entry(customer.id(), Direction::Credit, dec!(100)),
        ],
    ))
    .await
    .unwrap();

    assert_eq!(repo.balance(cash.id()).await.unwrap().amount(), dec!(100));
    assert_eq!(repo.balance(customer.id()).await.unwrap().amount(), dec!(100));

    let h = repo.history(cash.id(), 10).await.unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].direction(), Direction::Debit);
    assert_eq!(h[0].amount().amount(), dec!(100));
}

// --- INV6 --------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_idempotency_key_returns_same_id_and_does_not_double_post(pool: PgPool) {
    let repo = PgLedgerRepo::new(pool);
    let cash = account(1, AccountKind::Asset, false);
    let customer = account(2, AccountKind::Liability, false);
    repo.create_account(cash.clone()).await.unwrap();
    repo.create_account(customer.clone()).await.unwrap();

    let make_entries = || {
        vec![
            entry(cash.id(), Direction::Debit, dec!(50)),
            entry(customer.id(), Direction::Credit, dec!(50)),
        ]
    };

    // Two posts, different transaction ids, SAME idempotency key.
    let first = repo
        .post_transaction(tx(100, "dedupe-me", make_entries()))
        .await
        .unwrap();
    let second = repo
        .post_transaction(tx(200, "dedupe-me", make_entries()))
        .await
        .unwrap();

    assert_eq!(first, second, "duplicate-key replay must return the original id");
    assert_eq!(repo.balance(cash.id()).await.unwrap().amount(), dec!(50));
    assert_eq!(repo.history(cash.id(), 10).await.unwrap().len(), 1);
}

// --- INV5 --------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn non_negative_account_rejects_post_that_would_drive_it_negative(pool: PgPool) {
    let repo = PgLedgerRepo::new(pool);
    let strict_asset = account(1, AccountKind::Asset, false);
    let counter = account(2, AccountKind::Liability, true);
    repo.create_account(strict_asset.clone()).await.unwrap();
    repo.create_account(counter.clone()).await.unwrap();

    // Asset credit decreases the balance; start at 0 → would land at -100.
    let err = repo
        .post_transaction(tx(
            100,
            "would-go-neg",
            vec![
                entry(strict_asset.id(), Direction::Credit, dec!(100)),
                entry(counter.id(), Direction::Debit, dec!(100)),
            ],
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, LedgerError::AccountWouldGoNegative(_)));

    // Nothing persisted.
    assert_eq!(
        repo.balance(strict_asset.id()).await.unwrap().amount(),
        dec!(0)
    );
    assert_eq!(repo.history(strict_asset.id(), 10).await.unwrap().len(), 0);
}

// --- INV2 (the role half) ----------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn ledgercore_app_role_cannot_update_entries(pool: PgPool) {
    // Post something so there's at least one row to try to mutate.
    let repo = PgLedgerRepo::new(pool.clone());
    let cash = account(1, AccountKind::Asset, false);
    let customer = account(2, AccountKind::Liability, false);
    repo.create_account(cash.clone()).await.unwrap();
    repo.create_account(customer.clone()).await.unwrap();
    repo.post_transaction(tx(
        100,
        "for-update-test",
        vec![
            entry(cash.id(), Direction::Debit, dec!(1)),
            entry(customer.id(), Direction::Credit, dec!(1)),
        ],
    ))
    .await
    .unwrap();

    // Drop down to the restricted role on a single connection.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET ROLE ledgercore_app")
        .execute(&mut *conn)
        .await
        .unwrap();

    let update_err = sqlx::query("UPDATE entries SET amount = 0")
        .execute(&mut *conn)
        .await
        .unwrap_err();
    // 42501 = insufficient_privilege.
    match update_err {
        sqlx::Error::Database(db_err) => {
            assert_eq!(db_err.code().as_deref(), Some("42501"));
        }
        other => panic!("expected database error, got {other:?}"),
    }

    let delete_err = sqlx::query("DELETE FROM entries")
        .execute(&mut *conn)
        .await
        .unwrap_err();
    match delete_err {
        sqlx::Error::Database(db_err) => {
            assert_eq!(db_err.code().as_deref(), Some("42501"));
        }
        other => panic!("expected database error, got {other:?}"),
    }
}

// --- concurrency -------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn concurrent_posts_on_same_account_have_no_lost_updates(pool: PgPool) {
    let repo = Arc::new(PgLedgerRepo::new(pool));
    let cash = account(1, AccountKind::Asset, false);
    let customer = account(2, AccountKind::Liability, false);
    repo.create_account(cash.clone()).await.unwrap();
    repo.create_account(customer.clone()).await.unwrap();

    const N: usize = 20;
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let repo = repo.clone();
        let cash_id = cash.id();
        let cust_id = customer.id();
        handles.push(tokio::spawn(async move {
            repo.post_transaction(tx(
                1000 + i as u128,
                &format!("concurrent-{i}"),
                vec![
                    entry(cash_id, Direction::Debit, dec!(1)),
                    entry(cust_id, Direction::Credit, dec!(1)),
                ],
            ))
            .await
            .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    // Final balance is exactly N — no lost updates from interleaved posts.
    assert_eq!(
        repo.balance(cash.id()).await.unwrap().amount(),
        Decimal::from(N as i64)
    );
    assert_eq!(
        repo.balance(customer.id()).await.unwrap().amount(),
        Decimal::from(N as i64)
    );
    assert_eq!(repo.history(cash.id(), 1000).await.unwrap().len(), N);
}
