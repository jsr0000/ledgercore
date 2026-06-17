//! `Transaction` and the balanced-posting check (INV1). The conceptual
//! heart of M0 — see `docs/DESIGN-M0.md` §4 for the written-out check.
//!
//! `Transaction::new` validates entries during construction and stores
//! them in a private field, so an unbalanced `Transaction` cannot exist
//! as a value. Later layers can rely on this without re-checking.

use std::collections::HashSet;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::{AccountId, Currency, Direction, Entry, IdempotencyKey, Money, TransactionId};

/// A balanced set of postings carrying an idempotency key and a timestamp.
///
/// Construction-time invariants (enforced by [`Transaction::new`]):
///
/// 1. At least two entries.
/// 2. All entries share a single [`Currency`].
/// 3. **INV1**: sum of debits == sum of credits.
/// 4. At least two distinct accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    id: TransactionId,
    idempotency_key: IdempotencyKey,
    entries: Vec<Entry>,
    occurred_at: OffsetDateTime,
}

impl Transaction {
    /// Construct a transaction. `occurred_at` is supplied by the caller;
    /// the domain does not read the system clock.
    pub fn new(
        id: TransactionId,
        idempotency_key: IdempotencyKey,
        entries: Vec<Entry>,
        occurred_at: OffsetDateTime,
    ) -> Result<Self, TransactionError> {
        validate_entries(&entries)?;
        Ok(Self { id, idempotency_key, entries, occurred_at })
    }

    /// Identity of the transaction.
    pub fn id(&self) -> TransactionId {
        self.id
    }

    /// Caller-supplied dedupe token.
    pub fn idempotency_key(&self) -> &IdempotencyKey {
        &self.idempotency_key
    }

    /// The entries. Always non-empty, single-currency, balanced.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// When the transaction occurred.
    pub fn occurred_at(&self) -> OffsetDateTime {
        self.occurred_at
    }

    /// The single currency of this transaction.
    pub fn currency(&self) -> Currency {
        self.entries[0].amount().currency()
    }
}

/// One pass over the entries: precondition checks, then totals, then
/// the INV1 comparison and distinct-account count.
fn validate_entries(entries: &[Entry]) -> Result<(), TransactionError> {
    // Empty list → not a transaction.
    let first = entries.first().ok_or(TransactionError::NoEntries)?;

    // A single entry can't balance (Entry amounts are strictly positive),
    // but a precise error is more useful than the generic Unbalanced one.
    if entries.len() < 2 {
        return Err(TransactionError::SingleEntry);
    }

    // All entries must agree on currency; the transaction's currency is
    // the currency of the first entry.
    let currency = first.amount().currency();

    let mut debits = Money::zero(currency);
    let mut credits = Money::zero(currency);
    let mut accounts: HashSet<AccountId> = HashSet::with_capacity(entries.len());

    for e in entries {
        if e.amount().currency() != currency {
            return Err(TransactionError::MixedCurrencies);
        }
        accounts.insert(e.account());

        // try_add's only failure mode is currency mismatch, which we just
        // ruled out for this entry — so .expect is the honest spelling.
        match e.direction() {
            Direction::Debit => {
                debits = debits.try_add(e.amount()).expect("currencies match");
            }
            Direction::Credit => {
                credits = credits.try_add(e.amount()).expect("currencies match");
            }
        }
    }

    // INV1.
    if debits.amount() != credits.amount() {
        return Err(TransactionError::Unbalanced {
            debits: debits.amount(),
            credits: credits.amount(),
        });
    }

    // A balanced same-account "transaction" is degenerate.
    if accounts.len() < 2 {
        return Err(TransactionError::SingleAccount);
    }

    Ok(())
}

/// Errors raised when constructing a [`Transaction`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TransactionError {
    /// No entries supplied.
    #[error("transaction has no entries")]
    NoEntries,

    /// Only one entry supplied.
    #[error("transaction has only one entry; need at least one debit and one credit")]
    SingleEntry,

    /// All entries touch the same account.
    #[error("transaction entries all belong to the same account")]
    SingleAccount,

    /// Entries span more than one currency.
    #[error("transaction entries use multiple currencies")]
    MixedCurrencies,

    /// INV1 violated: sum of debits ≠ sum of credits.
    #[error("transaction is unbalanced: debits ({debits}) != credits ({credits})")]
    Unbalanced {
        /// Sum of all debit-side amounts.
        debits: Decimal,
        /// Sum of all credit-side amounts.
        credits: Decimal,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::macros::datetime;
    use uuid::Uuid;

    fn acct(n: u128) -> AccountId {
        AccountId::new(Uuid::from_u128(n))
    }

    fn tx_id() -> TransactionId {
        TransactionId::new(Uuid::from_u128(0xCAFE))
    }

    fn key() -> IdempotencyKey {
        IdempotencyKey::new("test-key").unwrap()
    }

    fn at() -> OffsetDateTime {
        datetime!(2026-06-17 10:00:00 UTC)
    }

    fn debit(account: AccountId, amount: Decimal) -> Entry {
        Entry::new(account, Direction::Debit, Money::new(amount, Currency::Usd)).unwrap()
    }

    fn credit(account: AccountId, amount: Decimal) -> Entry {
        Entry::new(account, Direction::Credit, Money::new(amount, Currency::Usd)).unwrap()
    }

    #[test]
    fn balanced_two_entry_transaction_is_accepted() {
        let entries = vec![debit(acct(1), dec!(100.00)), credit(acct(2), dec!(100.00))];
        let tx = Transaction::new(tx_id(), key(), entries, at()).unwrap();
        assert_eq!(tx.entries().len(), 2);
        assert_eq!(tx.currency(), Currency::Usd);
        assert_eq!(tx.occurred_at(), at());
        assert_eq!(tx.id(), tx_id());
        assert_eq!(tx.idempotency_key(), &key());
    }

    #[test]
    fn balanced_four_entry_transaction_is_accepted() {
        let entries = vec![
            debit(acct(1), dec!(30)),
            debit(acct(1), dec!(70)),
            credit(acct(2), dec!(40)),
            credit(acct(3), dec!(60)),
        ];
        Transaction::new(tx_id(), key(), entries, at()).unwrap();
    }

    #[test]
    fn empty_entries_rejected() {
        let err = Transaction::new(tx_id(), key(), vec![], at()).unwrap_err();
        assert_eq!(err, TransactionError::NoEntries);
    }

    #[test]
    fn single_entry_rejected() {
        let entries = vec![debit(acct(1), dec!(100))];
        let err = Transaction::new(tx_id(), key(), entries, at()).unwrap_err();
        assert_eq!(err, TransactionError::SingleEntry);
    }

    #[test]
    fn single_account_rejected() {
        let entries = vec![debit(acct(1), dec!(50)), credit(acct(1), dec!(50))];
        let err = Transaction::new(tx_id(), key(), entries, at()).unwrap_err();
        assert_eq!(err, TransactionError::SingleAccount);
    }

    #[test]
    fn unbalanced_rejected_with_totals() {
        let entries = vec![debit(acct(1), dec!(100)), credit(acct(2), dec!(99.99))];
        let err = Transaction::new(tx_id(), key(), entries, at()).unwrap_err();
        assert_eq!(
            err,
            TransactionError::Unbalanced {
                debits: dec!(100),
                credits: dec!(99.99),
            }
        );
    }

    #[test]
    fn precision_difference_is_unbalanced() {
        // The classic f64 trap. With Decimal we reject any delta exactly.
        let entries = vec![
            debit(acct(1), dec!(0.1)),
            debit(acct(1), dec!(0.2)),
            credit(acct(2), dec!(0.30001)),
        ];
        let err = Transaction::new(tx_id(), key(), entries, at()).unwrap_err();
        assert!(matches!(err, TransactionError::Unbalanced { .. }));
    }

    #[test]
    fn perfect_decimal_sum_is_accepted() {
        let entries = vec![
            debit(acct(1), dec!(0.1)),
            debit(acct(1), dec!(0.2)),
            credit(acct(2), dec!(0.3)),
        ];
        Transaction::new(tx_id(), key(), entries, at()).unwrap();
    }

    // Property tests for INV1. The hand-picked tests above pin specific
    // cases; these confirm the invariant holds across a randomised input
    // space and that the rejection error is the right variant.
    mod proptest_inv1 {
        use super::*;
        use proptest::prelude::*;

        // Mantissa 1..10_000_000 and scale 0..=4 keep sums well within
        // Decimal range and shrink to small, legible counter-examples.
        fn positive_decimal() -> impl Strategy<Value = Decimal> {
            (1i64..10_000_000, 0u32..=4).prop_map(|(m, s)| Decimal::new(m, s))
        }

        // n debit entries on n distinct accounts + one credit on a fresh
        // account totalling the debits. n in 2..=10, so the result has
        // 3..=11 entries on 3..=11 distinct accounts.
        fn balanced_entries() -> impl Strategy<Value = Vec<Entry>> {
            (2usize..=10)
                .prop_flat_map(|n| prop::collection::vec(positive_decimal(), n))
                .prop_map(|amounts| {
                    let total: Decimal = amounts.iter().sum();
                    let n = amounts.len();
                    let mut entries: Vec<Entry> = amounts
                        .into_iter()
                        .enumerate()
                        .map(|(i, amt)| {
                            Entry::new(
                                AccountId::new(Uuid::from_u128(i as u128)),
                                Direction::Debit,
                                Money::new(amt, Currency::Usd),
                            )
                            .unwrap()
                        })
                        .collect();
                    entries.push(
                        Entry::new(
                            AccountId::new(Uuid::from_u128(n as u128)),
                            Direction::Credit,
                            Money::new(total, Currency::Usd),
                        )
                        .unwrap(),
                    );
                    entries
                })
        }

        // Take a balanced set and perturb the first entry's amount by a
        // strictly positive delta. The result is guaranteed unbalanced
        // (debits exceed credits by `delta`).
        fn unbalanced_entries() -> impl Strategy<Value = Vec<Entry>> {
            (balanced_entries(), positive_decimal()).prop_map(|(mut entries, delta)| {
                let first = entries.remove(0);
                let new_amount = first.amount().amount() + delta;
                let perturbed = Entry::new(
                    first.account(),
                    first.direction(),
                    Money::new(new_amount, first.amount().currency()),
                )
                .unwrap();
                entries.insert(0, perturbed);
                entries
            })
        }

        proptest! {
            #[test]
            fn balanced_random_transactions_are_accepted(entries in balanced_entries()) {
                Transaction::new(tx_id(), key(), entries, at())
                    .expect("balanced generator should always produce a valid transaction");
            }

            #[test]
            fn unbalanced_random_transactions_are_rejected(entries in unbalanced_entries()) {
                let err = Transaction::new(tx_id(), key(), entries, at()).unwrap_err();
                let is_unbalanced = matches!(err, TransactionError::Unbalanced { .. });
                prop_assert!(is_unbalanced, "expected Unbalanced, got {:?}", err);
            }
        }
    }
}
