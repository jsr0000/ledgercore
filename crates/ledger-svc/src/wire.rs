//! Wire ⇄ domain conversions. Pure: no IO, no async. The boundary
//! between protobuf-shaped data and the strict domain types.

use std::str::FromStr;

use app::{Clock, IdGen};
use domain::{
    AccountId, Currency, Direction, Entry, EntryError, IdempotencyKey, IdempotencyKeyError,
    Money, MoneyError, Transaction, TransactionError,
};
use prost_types::Timestamp;
use rust_decimal::Decimal;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::proto;

// --- request → domain --------------------------------------------------------

/// Convert a `PostTransactionRequest` into a domain [`Transaction`].
pub fn to_domain_transaction(
    req: proto::PostTransactionRequest,
    clock: &dyn Clock,
    ids: &dyn IdGen,
) -> Result<Transaction, WireError> {
    let key = IdempotencyKey::new(req.idempotency_key)?;
    let entries: Vec<Entry> = req
        .entries
        .into_iter()
        .map(to_domain_entry)
        .collect::<Result<_, _>>()?;
    let occurred_at = match req.occurred_at {
        Some(ts) => parse_timestamp(ts)?,
        None => clock.now(),
    };
    let id = ids.new_transaction_id();
    Transaction::new(id, key, entries, occurred_at).map_err(WireError::Transaction)
}

/// Convert a wire `Entry` into a domain [`Entry`].
pub fn to_domain_entry(e: proto::Entry) -> Result<Entry, WireError> {
    let account = parse_account_id(&e.account_id)?;
    let direction = wire_direction(e.direction())?;
    let amount = to_money(e.amount.ok_or(WireError::Missing("entry.amount"))?)?;
    Entry::new(account, direction, amount).map_err(WireError::Entry)
}

/// Convert a wire `Money` into a domain [`Money`].
pub fn to_money(m: proto::Money) -> Result<Money, WireError> {
    let currency = wire_currency(m.currency())?;
    let amount = parse_decimal(&m.amount)?;
    Ok(Money::new(amount, currency))
}

fn wire_direction(d: proto::Direction) -> Result<Direction, WireError> {
    match d {
        proto::Direction::Debit => Ok(Direction::Debit),
        proto::Direction::Credit => Ok(Direction::Credit),
        proto::Direction::Unspecified => Err(WireError::UnspecifiedEnum("direction")),
    }
}

fn wire_currency(c: proto::Currency) -> Result<Currency, WireError> {
    match c {
        proto::Currency::Usd => Ok(Currency::Usd),
        proto::Currency::Unspecified => Err(WireError::UnspecifiedEnum("currency")),
    }
}

/// Parse a string UUID into [`AccountId`].
pub fn parse_account_id(s: &str) -> Result<AccountId, WireError> {
    Uuid::parse_str(s)
        .map(AccountId::new)
        .map_err(|e| WireError::InvalidUuid {
            field: "account_id",
            err: e.to_string(),
        })
}

/// Parse a string decimal.
pub fn parse_decimal(s: &str) -> Result<Decimal, WireError> {
    Decimal::from_str(s).map_err(|e| WireError::InvalidDecimal {
        field: "amount",
        err: e.to_string(),
    })
}

/// Convert a `google.protobuf.Timestamp` into `OffsetDateTime`.
pub fn parse_timestamp(ts: Timestamp) -> Result<OffsetDateTime, WireError> {
    let nanos = i128::from(ts.seconds) * 1_000_000_000 + i128::from(ts.nanos);
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|e| WireError::InvalidTimestamp(e.to_string()))
}

// --- domain → response (infallible) -----------------------------------------

/// Convert a domain [`Money`] to wire `Money`.
pub fn from_money(m: &Money) -> proto::Money {
    proto::Money {
        amount: m.amount().to_string(),
        currency: from_currency(m.currency()) as i32,
    }
}

/// Convert a domain [`Entry`] to wire `Entry`.
pub fn from_entry(e: &Entry) -> proto::Entry {
    proto::Entry {
        account_id: e.account().to_string(),
        direction: from_direction(e.direction()) as i32,
        amount: Some(from_money(e.amount())),
    }
}

fn from_currency(c: Currency) -> proto::Currency {
    match c {
        Currency::Usd => proto::Currency::Usd,
    }
}

fn from_direction(d: Direction) -> proto::Direction {
    match d {
        Direction::Debit => proto::Direction::Debit,
        Direction::Credit => proto::Direction::Credit,
    }
}

// --- error -----------------------------------------------------------------

/// Errors raised when translating wire types into domain types.
#[derive(thiserror::Error, Debug)]
pub enum WireError {
    /// A required message field was missing.
    #[error("missing required field: {0}")]
    Missing(&'static str),

    /// A UUID string could not be parsed.
    #[error("invalid uuid in field {field}: {err}")]
    InvalidUuid {
        /// The named field carrying the bad UUID.
        field: &'static str,
        /// Parser's error message.
        err: String,
    },

    /// A decimal string could not be parsed.
    #[error("invalid decimal in field {field}: {err}")]
    InvalidDecimal {
        /// The named field carrying the bad decimal.
        field: &'static str,
        /// Parser's error message.
        err: String,
    },

    /// A timestamp was out of range.
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),

    /// An enum field arrived as proto3 `UNSPECIFIED` (= 0).
    #[error("enum is unspecified for field: {0}")]
    UnspecifiedEnum(&'static str),

    /// Forwarded from [`IdempotencyKey::new`].
    #[error(transparent)]
    Idempotency(#[from] IdempotencyKeyError),

    /// Forwarded from [`Entry::new`].
    #[error(transparent)]
    Entry(EntryError),

    /// Forwarded from [`Money::positive`].
    #[error(transparent)]
    Money(MoneyError),

    /// Forwarded from [`Transaction::new`].
    #[error(transparent)]
    Transaction(TransactionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn money_round_trip_usd() {
        let m = to_money(proto::Money {
            amount: "100.00".into(),
            currency: proto::Currency::Usd as i32,
        })
        .unwrap();
        assert_eq!(m.amount(), dec!(100.00));
        assert_eq!(m.currency(), Currency::Usd);

        let back = from_money(&m);
        assert_eq!(back.amount, "100.00");
        assert_eq!(back.currency, proto::Currency::Usd as i32);
    }

    #[test]
    fn money_rejects_unspecified_currency() {
        let err = to_money(proto::Money {
            amount: "1".into(),
            currency: proto::Currency::Unspecified as i32,
        })
        .unwrap_err();
        assert!(matches!(err, WireError::UnspecifiedEnum("currency")));
    }

    #[test]
    fn money_rejects_unparseable_amount() {
        let err = to_money(proto::Money {
            amount: "twelve".into(),
            currency: proto::Currency::Usd as i32,
        })
        .unwrap_err();
        assert!(matches!(err, WireError::InvalidDecimal { .. }));
    }

    #[test]
    fn account_id_rejects_garbage() {
        let err = parse_account_id("not-a-uuid").unwrap_err();
        assert!(matches!(err, WireError::InvalidUuid { .. }));
    }

    #[test]
    fn timestamp_round_trip_via_unix_epoch() {
        let ts = Timestamp { seconds: 1_780_000_000, nanos: 123_456_789 };
        let dt = parse_timestamp(ts).unwrap();
        assert_eq!(dt.unix_timestamp(), 1_780_000_000);
        assert_eq!(dt.unix_timestamp_nanos() % 1_000_000_000, 123_456_789);
    }

    #[test]
    fn entry_rejects_missing_amount() {
        let err = to_domain_entry(proto::Entry {
            account_id: Uuid::from_u128(1).to_string(),
            direction: proto::Direction::Debit as i32,
            amount: None,
        })
        .unwrap_err();
        assert!(matches!(err, WireError::Missing("entry.amount")));
    }

    #[test]
    fn entry_rejects_unspecified_direction() {
        let err = to_domain_entry(proto::Entry {
            account_id: Uuid::from_u128(1).to_string(),
            direction: proto::Direction::Unspecified as i32,
            amount: Some(proto::Money {
                amount: "1".into(),
                currency: proto::Currency::Usd as i32,
            }),
        })
        .unwrap_err();
        assert!(matches!(err, WireError::UnspecifiedEnum("direction")));
    }
}
