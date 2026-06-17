//! `Account` aggregate. The rule "asset debit increases, liability credit
//! increases" lives here so callers don't have to remember it. See
//! `docs/DESIGN-M1.md` §3.

use rust_decimal::Decimal;

use crate::{AccountId, Currency, Direction, Money};

/// Whether an account holds value (asset) or owes it (liability).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountKind {
    /// Asset: debit increases the balance.
    Asset,
    /// Liability: credit increases the balance.
    Liability,
}

/// An account in the ledger. Carries the rules needed to apply an entry
/// and to enforce the non-negative constraint (INV5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    id: AccountId,
    kind: AccountKind,
    currency: Currency,
    allow_negative: bool,
}

impl Account {
    /// Construct an account.
    ///
    /// `allow_negative` is explicit rather than derived from `kind` so that
    /// individual accounts can override the textbook default (some treasury
    /// accounts are assets that must stay non-negative; some receivables
    /// are liabilities that legitimately dip below zero).
    pub const fn new(
        id: AccountId,
        kind: AccountKind,
        currency: Currency,
        allow_negative: bool,
    ) -> Self {
        Self { id, kind, currency, allow_negative }
    }

    /// The account's id.
    pub fn id(&self) -> AccountId {
        self.id
    }

    /// Asset or liability.
    pub fn kind(&self) -> AccountKind {
        self.kind
    }

    /// The currency of this account.
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Whether the balance is allowed to be negative.
    pub fn allow_negative(&self) -> bool {
        self.allow_negative
    }

    /// Apply one entry to the current balance and return the new balance.
    /// Errors if any currency disagrees with the account's currency.
    pub fn apply(
        &self,
        direction: Direction,
        amount: &Money,
        current: &Money,
    ) -> Result<Money, AccountError> {
        if amount.currency() != self.currency {
            return Err(AccountError::CurrencyMismatch {
                account: self.id,
                expected: self.currency,
                got: amount.currency(),
            });
        }
        if current.currency() != self.currency {
            return Err(AccountError::CurrencyMismatch {
                account: self.id,
                expected: self.currency,
                got: current.currency(),
            });
        }

        // Signed delta: increases on debit-for-asset / credit-for-liability,
        // decreases on the opposite pair.
        let delta = match (self.kind, direction) {
            (AccountKind::Asset, Direction::Debit)
            | (AccountKind::Liability, Direction::Credit) => amount.amount(),
            (AccountKind::Asset, Direction::Credit)
            | (AccountKind::Liability, Direction::Debit) => -amount.amount(),
        };

        Ok(Money::new(current.amount() + delta, self.currency))
    }

    /// Check that a projected post-write balance is allowed (INV5).
    pub fn check_post(&self, projected: &Money) -> Result<(), AccountError> {
        if projected.currency() != self.currency {
            return Err(AccountError::CurrencyMismatch {
                account: self.id,
                expected: self.currency,
                got: projected.currency(),
            });
        }
        if !self.allow_negative && projected.amount() < Decimal::ZERO {
            return Err(AccountError::WouldGoNegative {
                account: self.id,
                balance: projected.amount(),
            });
        }
        Ok(())
    }
}

/// Errors raised by [`Account::apply`] and [`Account::check_post`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    /// Currency on an amount or current balance didn't match the account.
    #[error("account {account} currency mismatch: expected {expected}, got {got}")]
    CurrencyMismatch {
        /// The account whose currency was violated.
        account: AccountId,
        /// The account's currency.
        expected: Currency,
        /// The offending currency.
        got: Currency,
    },

    /// Projected balance would be negative on an account that disallows it.
    #[error("account {account} would go negative: balance would be {balance}")]
    WouldGoNegative {
        /// The account.
        account: AccountId,
        /// The projected (negative) balance.
        balance: Decimal,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn asset() -> Account {
        Account::new(
            AccountId::new(Uuid::from_u128(1)),
            AccountKind::Asset,
            Currency::Usd,
            false,
        )
    }

    fn liability() -> Account {
        Account::new(
            AccountId::new(Uuid::from_u128(2)),
            AccountKind::Liability,
            Currency::Usd,
            false,
        )
    }

    fn money(d: Decimal) -> Money {
        Money::new(d, Currency::Usd)
    }

    #[test]
    fn asset_debit_increases_balance() {
        let new = asset()
            .apply(Direction::Debit, &money(dec!(10)), &money(dec!(50)))
            .unwrap();
        assert_eq!(new.amount(), dec!(60));
    }

    #[test]
    fn asset_credit_decreases_balance() {
        let new = asset()
            .apply(Direction::Credit, &money(dec!(10)), &money(dec!(50)))
            .unwrap();
        assert_eq!(new.amount(), dec!(40));
    }

    #[test]
    fn liability_credit_increases_balance() {
        let new = liability()
            .apply(Direction::Credit, &money(dec!(10)), &money(dec!(50)))
            .unwrap();
        assert_eq!(new.amount(), dec!(60));
    }

    #[test]
    fn liability_debit_decreases_balance() {
        let new = liability()
            .apply(Direction::Debit, &money(dec!(10)), &money(dec!(50)))
            .unwrap();
        assert_eq!(new.amount(), dec!(40));
    }

    #[test]
    fn check_post_rejects_negative_when_disallowed() {
        let err = asset().check_post(&money(dec!(-0.01))).unwrap_err();
        assert!(matches!(err, AccountError::WouldGoNegative { .. }));
    }

    #[test]
    fn check_post_accepts_negative_when_allowed() {
        let acct = Account::new(
            AccountId::new(Uuid::from_u128(3)),
            AccountKind::Asset,
            Currency::Usd,
            true,
        );
        acct.check_post(&money(dec!(-100))).unwrap();
    }

    #[test]
    fn check_post_accepts_zero() {
        asset().check_post(&money(dec!(0))).unwrap();
    }

    // Property: applying a direction and then its opposite returns to the
    // original balance. Encodes "debit and credit are inverses" generically
    // across kind and amount.
    mod proptest_apply {
        use super::*;
        use proptest::prelude::*;

        fn positive_decimal() -> impl Strategy<Value = Decimal> {
            (1i64..1_000_000, 0u32..=4).prop_map(|(m, s)| Decimal::new(m, s))
        }

        fn any_account() -> impl Strategy<Value = Account> {
            (
                prop_oneof![Just(AccountKind::Asset), Just(AccountKind::Liability)],
                any::<bool>(),
            )
                .prop_map(|(kind, allow_neg)| {
                    Account::new(
                        AccountId::new(Uuid::from_u128(99)),
                        kind,
                        Currency::Usd,
                        allow_neg,
                    )
                })
        }

        proptest! {
            #[test]
            fn apply_and_reverse_round_trips(
                account in any_account(),
                amount in positive_decimal(),
                start in -1_000_000i64..1_000_000,
            ) {
                let amt = Money::new(amount, Currency::Usd);
                let initial = Money::new(Decimal::from(start), Currency::Usd);
                let after = account.apply(Direction::Debit, &amt, &initial).unwrap();
                let restored = account.apply(Direction::Credit, &amt, &after).unwrap();
                prop_assert_eq!(restored.amount(), initial.amount());
            }
        }
    }
}
