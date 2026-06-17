//! ledgercore domain.
//!
//! Pure types and invariants for a double-entry ledger. No IO, no async, no
//! framework types. See `docs/DESIGN-M0.md` for the design and the explicit
//! list of invariants this crate establishes.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

mod money;

pub use money::{Currency, Money, MoneyError};
