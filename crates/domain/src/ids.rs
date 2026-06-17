//! Identifier newtypes — `AccountId`, `TransactionId`, `IdempotencyKey`.
//!
//! Two reasons these exist instead of plain `Uuid` / `String`:
//!
//! 1. **Type safety.** `fn transfer(from: AccountId, to: AccountId)` cannot
//!    be called with the arguments swapped against a `TransactionId`. The
//!    cost is zero at runtime.
//! 2. **Validation lives on the constructor.** Bad shapes (an empty
//!    idempotency key, an over-long one) are rejected at the moment of
//!    construction, not silently propagated.
//!
//! Note: the pure domain deliberately does **not** generate UUIDs.
//! `Uuid::new_v4()` reads randomness from the OS, which is IO. ID
//! generation lives in the application layer (see M2+) behind a port.
//! In M0 callers and tests pass UUIDs in explicitly.

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

/// Maximum allowed length, in UTF-8 bytes, for an [`IdempotencyKey`].
///
/// Chosen to be generous for caller-supplied identifiers (UUIDs, hashes,
/// composite strings) while still capping unbounded growth. Postgres column
/// width in M1 will mirror this.
pub const IDEMPOTENCY_KEY_MAX_LEN: usize = 128;

/// A caller-supplied dedupe token for a transaction.
///
/// Submitting the same transaction twice with the same `IdempotencyKey`
/// must yield exactly one posting (INV6). The *enforcement* of that
/// invariant is the persistence layer's job (M1); the *concept* — that a
/// transaction carries this token — is a domain concern, so the type lives
/// here and is mandatory on `Transaction`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Construct from a string, validating shape.
    ///
    /// Rejects:
    /// - the empty string,
    /// - strings longer than [`IDEMPOTENCY_KEY_MAX_LEN`] bytes.
    ///
    /// Deliberately permissive about content: callers may legitimately
    /// embed UUIDs, opaque tokens, or hex digests, and the domain has no
    /// business prescribing a character set.
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
    /// The empty string was supplied.
    #[error("idempotency key is empty")]
    Empty,

    /// The string exceeded [`IDEMPOTENCY_KEY_MAX_LEN`] bytes.
    #[error("idempotency key is {len} bytes, maximum is {max}")]
    TooLong {
        /// Actual length in bytes.
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
        let id = AccountId::new(raw);
        assert_eq!(id.as_uuid(), raw);
    }

    #[test]
    fn transaction_id_round_trips_uuid() {
        let raw = Uuid::from_u128(42);
        let id = TransactionId::new(raw);
        assert_eq!(id.as_uuid(), raw);
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
        assert_eq!(
            IdempotencyKey::new(""),
            Err(IdempotencyKeyError::Empty),
        );
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
        // Boundary case: equal to MAX_LEN is allowed.
        let s = "x".repeat(IDEMPOTENCY_KEY_MAX_LEN);
        assert!(IdempotencyKey::new(&s).is_ok());
    }

    #[test]
    fn idempotency_key_permits_uuid_shape() {
        // A common real-world choice.
        let s = Uuid::from_u128(0xABCDEF).to_string();
        assert!(IdempotencyKey::new(s).is_ok());
    }
}
