//! Postgres adapter implementing the ledger ports. See
//! `docs/DESIGN-M1.md` §5–§7.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use sqlx::PgPool;

/// `LedgerRepo` implementation backed by a Postgres connection pool.
pub struct PgLedgerRepo {
    pool: PgPool,
}

impl PgLedgerRepo {
    /// Construct from an existing pool. The caller owns connection-pool
    /// configuration; this crate stays free of policy.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying pool — useful for the recon CLI and tests
    /// that need to run ad-hoc queries.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
