//! Application layer: ports + use-cases. See `docs/DESIGN-M1.md` §4.
//!
//! The crate depends only on `domain`. Adapters (Postgres, gRPC, mock
//! chain) live in `infra-*` crates and implement these traits.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

mod ledger;

pub use ledger::{LedgerError, LedgerRepo};
