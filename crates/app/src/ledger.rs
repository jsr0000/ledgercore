//! The `LedgerRepo` port and its error type.

use async_trait::async_trait;
use domain::{Account, AccountId, Currency, Entry, Money, Transaction, TransactionId};

/// The persistence contract for the ledger.
///
/// Implementations must guarantee:
///
/// - **INV2**: `entries` are never updated or deleted.
/// - **INV3 / INV4 / INV5 / INV6**: enforced inside `post_transaction`.
///
/// `post_transaction` is observably idempotent: re-posting with the
/// same idempotency key returns the original transaction's id without
/// inserting new entries.
#[async_trait]
pub trait LedgerRepo: Send + Sync {
    /// Create a new account. Errors if an account with the same id
    /// already exists.
    async fn create_account(&self, account: Account) -> Result<(), LedgerError>;

    /// Post a transaction atomically. Returns the id of the persisted
    /// transaction — the input's id on a first post, the original id
    /// on a duplicate-key replay.
    async fn post_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionId, LedgerError>;

    /// Current balance of an account.
    async fn balance(&self, account: AccountId) -> Result<Money, LedgerError>;

    /// Most-recent entries for an account, newest first, up to `limit`.
    async fn history(
        &self,
        account: AccountId,
        limit: u32,
    ) -> Result<Vec<Entry>, LedgerError>;
}

/// Errors returned by [`LedgerRepo`].
#[derive(thiserror::Error, Debug)]
pub enum LedgerError {
    /// A referenced account doesn't exist.
    #[error("account not found: {0}")]
    AccountNotFound(AccountId),

    /// `create_account` called with an id that already exists.
    #[error("account already exists: {0}")]
    AccountAlreadyExists(AccountId),

    /// Posting would drive a non-negative account below zero (INV5).
    #[error("account {0} would go negative")]
    AccountWouldGoNegative(AccountId),

    /// A currency on the transaction didn't match the account's currency.
    #[error("account {account} currency mismatch: expected {expected}, got {got}")]
    CurrencyMismatch {
        /// The account whose currency was violated.
        account: AccountId,
        /// The account's currency.
        expected: Currency,
        /// The offending currency on the entry.
        got: Currency,
    },

    /// Underlying storage failure (sqlx, IO, etc).
    #[error("storage error")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}
