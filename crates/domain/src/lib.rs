//! ledgercore domain.
//!
//! Pure types and invariants for a double-entry ledger. No IO, no async, no
//! framework types. See `docs/DESIGN-M0.md` for the design and the explicit
//! list of invariants this crate establishes.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

// Modules are introduced one at a time in subsequent commits, alongside the
// types they define. See M0 design doc for the order.
