//! Identifier newtypes. UUIDs are wrapped (never generated here — that's
//! IO and lives in the app layer). See `docs/DESIGN-M0.md` §3.4.

use uuid::Uuid;

/// Identifier of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(Uuid);

impl AccountId {
    /// Wrap a pre-existing UUID.
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// The inner UUID.
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for AccountId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Identifier of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransactionId(Uuid);

impl TransactionId {
    /// Wrap a pre-existing UUID.
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// The inner UUID.
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for TransactionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Maximum length, in UTF-8 bytes, of an [`IdempotencyKey`]. Postgres
/// column width in M1 will mirror this.
pub const IDEMPOTENCY_KEY_MAX_LEN: usize = 128;

/// Caller-supplied dedupe token for a transaction. INV6 (exactly-once
/// posting per key) is enforced in M1 persistence; the type lives here
/// because the *concept* is domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Construct from a string. Rejects empty input and anything longer
    /// than [`IDEMPOTENCY_KEY_MAX_LEN`].
    pub fn new(s: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let s = s.into();
        if s.is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if s.len() > IDEMPOTENCY_KEY_MAX_LEN {
            return Err(IdempotencyKeyError::TooLong {
                len: s.len(),
                max: IDEMPOTENCY_KEY_MAX_LEN,
            });
        }
        Ok(Self(s))
    }

    /// The raw string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Errors raised when constructing an [`IdempotencyKey`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyKeyError {
    /// Empty string supplied.
    #[error("idempotency key is empty")]
    Empty,

    /// Input exceeded [`IDEMPOTENCY_KEY_MAX_LEN`] bytes.
    #[error("idempotency key is {len} bytes, maximum is {max}")]
    TooLong {
        /// Actual byte length.
        len: usize,
        /// Configured maximum.
        max: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_id_round_trips_uuid() {
        let raw = Uuid::from_u128(0xDEAD_BEEF);
        assert_eq!(AccountId::new(raw).as_uuid(), raw);
    }

    #[test]
    fn transaction_id_round_trips_uuid() {
        let raw = Uuid::from_u128(42);
        assert_eq!(TransactionId::new(raw).as_uuid(), raw);
    }

    #[test]
    fn ids_display_as_their_uuid() {
        let raw = Uuid::from_u128(0);
        assert_eq!(format!("{}", AccountId::new(raw)), raw.to_string());
        assert_eq!(format!("{}", TransactionId::new(raw)), raw.to_string());
    }

    #[test]
    fn idempotency_key_accepts_typical_input() {
        let k = IdempotencyKey::new("order-2026-06-17-abc123").unwrap();
        assert_eq!(k.as_str(), "order-2026-06-17-abc123");
    }

    #[test]
    fn idempotency_key_rejects_empty() {
        assert_eq!(IdempotencyKey::new(""), Err(IdempotencyKeyError::Empty));
    }

    #[test]
    fn idempotency_key_rejects_too_long() {
        let s = "x".repeat(IDEMPOTENCY_KEY_MAX_LEN + 1);
        assert_eq!(
            IdempotencyKey::new(&s),
            Err(IdempotencyKeyError::TooLong {
                len: IDEMPOTENCY_KEY_MAX_LEN + 1,
                max: IDEMPOTENCY_KEY_MAX_LEN,
            }),
        );
    }

    #[test]
    fn idempotency_key_accepts_exactly_max_len() {
        let s = "x".repeat(IDEMPOTENCY_KEY_MAX_LEN);
        assert!(IdempotencyKey::new(&s).is_ok());
    }

    #[test]
    fn idempotency_key_permits_uuid_shape() {
        let s = Uuid::from_u128(0xABCDEF).to_string();
        assert!(IdempotencyKey::new(s).is_ok());
    }
}
