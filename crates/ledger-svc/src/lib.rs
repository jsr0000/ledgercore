//! ledger-svc — gRPC server exposing the Ledger port. See
//! `docs/DESIGN-M2.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

/// Generated protobuf and tonic code. The build script writes this
/// module into OUT_DIR; we re-include it here so the rest of the crate
/// can refer to it via `ledger_svc::proto::...`.
pub mod proto {
    #![allow(missing_docs, clippy::all)]
    tonic::include_proto!("ledgercore.v1");
}

pub mod wire;
