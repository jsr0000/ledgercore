//! Concrete adapters for the `Clock` and `IdGen` ports. Live here
//! rather than in `app` because they read OS clocks / OS randomness,
//! which is IO.

use app::{Clock, IdGen};
use domain::TransactionId;
use time::OffsetDateTime;
use uuid::Uuid;

/// Reads the OS wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// Generates random UUID v4 transaction ids.
pub struct UuidV4Gen;

impl IdGen for UuidV4Gen {
    fn new_transaction_id(&self) -> TransactionId {
        TransactionId::new(Uuid::new_v4())
    }
}
