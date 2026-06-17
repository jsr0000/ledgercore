//! `WireError` and `LedgerError` → `tonic::Status`. The mapping table
//! from `docs/DESIGN-M2.md` §6 lives here as a single function each.
//! Tests in this file mirror the table — change the policy, change the
//! test, never one without the other.

use app::LedgerError;
use tonic::{Code, Status};

use crate::wire::WireError;

/// Map a [`WireError`] to a gRPC [`Status`].
///
/// Every wire error is a caller mistake (malformed input, missing
/// field, business-rule violation that the *type* caught), so all
/// variants surface as `INVALID_ARGUMENT`.
pub fn wire_to_status(e: WireError) -> Status {
    Status::new(Code::InvalidArgument, e.to_string())
}

/// Map a [`LedgerError`] to a gRPC [`Status`].
///
/// `Storage` errors deliberately scrub the inner error from the
/// wire message; the full chain is recorded in the tracing span.
pub fn ledger_to_status(e: LedgerError) -> Status {
    let code = match &e {
        LedgerError::AccountNotFound(_) => Code::NotFound,
        LedgerError::AccountAlreadyExists(_) => Code::AlreadyExists,
        LedgerError::AccountWouldGoNegative(_) => Code::FailedPrecondition,
        LedgerError::CurrencyMismatch { .. } => Code::InvalidArgument,
        LedgerError::Storage(_) => Code::Internal,
    };
    let message = match &e {
        LedgerError::Storage(_) => "storage error".to_string(),
        other => other.to_string(),
    };
    Status::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{AccountId, Currency};
    use uuid::Uuid;

    fn an_account() -> AccountId {
        AccountId::new(Uuid::from_u128(1))
    }

    // Table-driven test mirroring DESIGN-M2.md §6. Adding a LedgerError
    // variant requires both updating the mapping AND adding a row here.
    #[test]
    fn ledger_error_mapping_matches_table() {
        let cases: Vec<(LedgerError, Code)> = vec![
            (LedgerError::AccountNotFound(an_account()), Code::NotFound),
            (LedgerError::AccountAlreadyExists(an_account()), Code::AlreadyExists),
            (
                LedgerError::AccountWouldGoNegative(an_account()),
                Code::FailedPrecondition,
            ),
            (
                LedgerError::CurrencyMismatch {
                    account: an_account(),
                    expected: Currency::Usd,
                    got: Currency::Usd,
                },
                Code::InvalidArgument,
            ),
            (
                LedgerError::Storage(Box::new(std::io::Error::other("boom"))),
                Code::Internal,
            ),
        ];
        for (err, expected) in cases {
            let status = ledger_to_status(err);
            assert_eq!(status.code(), expected);
        }
    }

    #[test]
    fn storage_status_message_does_not_leak_inner_error() {
        let status = ledger_to_status(LedgerError::Storage(Box::new(
            std::io::Error::other("sensitive sqlx detail"),
        )));
        assert!(!status.message().contains("sensitive sqlx detail"));
        assert_eq!(status.message(), "storage error");
    }

    #[test]
    fn wire_errors_map_to_invalid_argument() {
        let cases = vec![
            WireError::Missing("entry.amount"),
            WireError::InvalidUuid {
                field: "account_id",
                err: "x".into(),
            },
            WireError::InvalidDecimal {
                field: "amount",
                err: "x".into(),
            },
            WireError::InvalidTimestamp("x".into()),
            WireError::UnspecifiedEnum("currency"),
        ];
        for err in cases {
            assert_eq!(wire_to_status(err).code(), Code::InvalidArgument);
        }
    }
}
