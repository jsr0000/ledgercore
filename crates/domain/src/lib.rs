//! ledgercore domain.
//!
//! Pure types and invariants for a double-entry ledger. No IO, no async, no
//! framework types. See `docs/DESIGN-M0.md` for the design and the explicit
//! list of invariants this crate establishes.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

mod entry;
mod ids;
mod money;
mod transaction;

pub use entry::{Direction, Entry, EntryError};
pub use ids::{
    AccountId, IdempotencyKey, IdempotencyKeyError, TransactionId, IDEMPOTENCY_KEY_MAX_LEN,
};
pub use money::{Currency, Money, MoneyError};
pub use transaction::{Transaction, TransactionError};
