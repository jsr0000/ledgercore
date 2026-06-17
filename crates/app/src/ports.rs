//! Side-effecting ports the service layer needs. Implementations live
//! in adapter crates (`ledger-svc` provides the real ones; tests provide
//! fakes).

use domain::TransactionId;
use time::OffsetDateTime;

/// Wall-clock reader. Sync because reading the clock doesn't block.
pub trait Clock: Send + Sync {
    /// Current UTC time.
    fn now(&self) -> OffsetDateTime;
}

/// Identifier generator. Sync because UUID generation doesn't block.
pub trait IdGen: Send + Sync {
    /// A fresh transaction id.
    fn new_transaction_id(&self) -> TransactionId;
}
