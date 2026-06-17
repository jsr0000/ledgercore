//! `LedgerRepo` implementation for `PgLedgerRepo`. The conceptual core
//! of M1 — see `docs/DESIGN-M1.md` §7.

use std::collections::BTreeSet;

use app::{LedgerError, LedgerRepo};
use async_trait::async_trait;
use domain::{
    Account, AccountError, AccountId, Entry, Money, Transaction, TransactionId,
};
use uuid::Uuid;

use crate::types::{DbAccountKind, DbCurrency, DbDirection};
use crate::PgLedgerRepo;

#[async_trait]
impl LedgerRepo for PgLedgerRepo {
    async fn create_account(&self, account: Account) -> Result<(), LedgerError> {
        let kind = DbAccountKind::from(account.kind());
        let currency = DbCurrency::from(account.currency());

        let res = sqlx::query!(
            r#"INSERT INTO accounts (id, kind, currency, allow_negative)
               VALUES ($1, $2, $3, $4)"#,
            account.id().as_uuid(),
            kind as DbAccountKind,
            currency as DbCurrency,
            account.allow_negative(),
        )
        .execute(&self.pool)
        .await;

        match res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Err(LedgerError::AccountAlreadyExists(account.id()))
            }
            Err(e) => Err(storage(e)),
        }
    }

    async fn post_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionId, LedgerError> {
        let mut conn = self.pool.begin().await.map_err(storage)?;

        // 1. Claim the idempotency key. ON CONFLICT DO NOTHING + RETURNING
        // gives us atomic "first poster wins" semantics: no race window
        // between SELECT-if-exists and INSERT.
        let claimed = sqlx::query_scalar!(
            r#"INSERT INTO transactions (id, idempotency_key, occurred_at)
               VALUES ($1, $2, $3)
               ON CONFLICT (idempotency_key) DO NOTHING
               RETURNING id"#,
            tx.id().as_uuid(),
            tx.idempotency_key().as_str(),
            tx.occurred_at(),
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(storage)?;

        if claimed.is_none() {
            // Duplicate key. Return the original transaction's id so the
            // caller can't observe which post "won".
            let existing = sqlx::query_scalar!(
                "SELECT id FROM transactions WHERE idempotency_key = $1",
                tx.idempotency_key().as_str(),
            )
            .fetch_one(&mut *conn)
            .await
            .map_err(storage)?;
            conn.commit().await.map_err(storage)?;
            return Ok(TransactionId::new(existing));
        }

        // 2. Distinct accounts touched, sorted. Locking in a deterministic
        // order is what prevents deadlocks between two concurrent posts
        // that touch the same set of accounts in different orders.
        let touched: BTreeSet<Uuid> =
            tx.entries().iter().map(|e| e.account().as_uuid()).collect();
        let touched_ids: Vec<Uuid> = touched.into_iter().collect();

        // 3. SELECT ... FOR UPDATE on every touched account. Holds row
        // locks until COMMIT/ROLLBACK; serialises any other poster that
        // touches the same accounts.
        let rows = sqlx::query!(
            r#"SELECT id,
                      kind     AS "kind: DbAccountKind",
                      currency AS "currency: DbCurrency",
                      allow_negative,
                      balance
               FROM accounts
               WHERE id = ANY($1)
               ORDER BY id
               FOR UPDATE"#,
            &touched_ids,
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(storage)?;

        if rows.len() != touched_ids.len() {
            let found: BTreeSet<Uuid> = rows.iter().map(|r| r.id).collect();
            for id in &touched_ids {
                if !found.contains(id) {
                    return Err(LedgerError::AccountNotFound(AccountId::new(*id)));
                }
            }
        }

        // 4. Build a working set of (Account, current_balance) pairs.
        // Vec rather than HashMap — small N, lookups are linear but cheap,
        // and the iteration order matches the lock order for clarity.
        let mut state: Vec<(Account, Money)> = rows
            .into_iter()
            .map(|r| {
                let currency = r.currency.into();
                let acct = Account::new(
                    AccountId::new(r.id),
                    r.kind.into(),
                    currency,
                    r.allow_negative,
                );
                let bal = Money::new(r.balance, currency);
                (acct, bal)
            })
            .collect();

        // 5. Apply each entry in submission order, updating the projected
        // balance in `state`. The DB hasn't seen any of this yet — these
        // are all in-memory projections.
        for e in tx.entries() {
            let idx = state
                .iter()
                .position(|(a, _)| a.id() == e.account())
                .ok_or(LedgerError::AccountNotFound(e.account()))?;
            let (acct, bal) = &state[idx];
            let new_bal = acct
                .apply(e.direction(), e.amount(), bal)
                .map_err(map_account_error)?;
            state[idx].1 = new_bal;
        }

        // 6. Once all entries are applied, verify each touched account's
        // final balance against INV5. Doing this AFTER applying all
        // entries lets a single transaction temporarily dip below zero
        // mid-flight as long as the final balance is valid.
        for (acct, bal) in &state {
            acct.check_post(bal).map_err(map_account_error)?;
        }

        // 7. Insert entries.
        for e in tx.entries() {
            let direction = DbDirection::from(e.direction());
            let currency = DbCurrency::from(e.amount().currency());
            sqlx::query!(
                r#"INSERT INTO entries
                       (transaction_id, account_id, direction, amount, currency)
                   VALUES ($1, $2, $3, $4, $5)"#,
                tx.id().as_uuid(),
                e.account().as_uuid(),
                direction as DbDirection,
                e.amount().amount(),
                currency as DbCurrency,
            )
            .execute(&mut *conn)
            .await
            .map_err(storage)?;
        }

        // 8. Persist the new balances.
        for (acct, bal) in &state {
            sqlx::query!(
                "UPDATE accounts SET balance = $1 WHERE id = $2",
                bal.amount(),
                acct.id().as_uuid(),
            )
            .execute(&mut *conn)
            .await
            .map_err(storage)?;
        }

        conn.commit().await.map_err(storage)?;
        Ok(tx.id())
    }

    async fn balance(&self, account: AccountId) -> Result<Money, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT balance, currency AS "currency: DbCurrency"
               FROM accounts
               WHERE id = $1"#,
            account.as_uuid(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;

        match row {
            Some(r) => Ok(Money::new(r.balance, r.currency.into())),
            None => Err(LedgerError::AccountNotFound(account)),
        }
    }

    async fn history(
        &self,
        _account: AccountId,
        _limit: u32,
    ) -> Result<Vec<Entry>, LedgerError> {
        // Implemented in the next commit (#17).
        unimplemented!()
    }
}

fn storage(e: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>) -> LedgerError {
    LedgerError::Storage(e.into())
}

fn map_account_error(e: AccountError) -> LedgerError {
    match e {
        AccountError::WouldGoNegative { account, .. } => {
            LedgerError::AccountWouldGoNegative(account)
        }
        AccountError::CurrencyMismatch {
            account,
            expected,
            got,
        } => LedgerError::CurrencyMismatch {
            account,
            expected,
            got,
        },
    }
}
