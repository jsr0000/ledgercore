//! `Direction` and `Entry` — the two halves of a posting.
//!
//! In double-entry accounting every transaction is a *set* of entries, each
//! entry touching one account in one direction with a strictly positive
//! amount. The direction (debit or credit) is *categorical*, not a sign:
//! whether a debit increases or decreases the account's balance depends on
//! the account type (asset vs liability), and that rule lives in one place
//! (M1's balance projection), not smuggled into every plus/minus.
//!
//! ## Why `Direction` is an enum, not a `+1 / -1`
//!
//! - Reading code: `Direction::Credit` is unambiguous;
//!   `Decimal::from(-1) * amount` invites the "I forgot to negate" bug.
//! - The balanced check (INV1) reads textbook-natural as
//!   `sum(debits) == sum(credits)`.
//! - Future symmetries — flipping all directions when posting a reversing
//!   transaction (M1) — are a single method, [`Direction::flip`], rather
//!   than a scatter of sign flips.
//!
//! ## Why `Entry::new` re-validates positivity
//!
//! `Money` itself permits zero or negative amounts (balances need that).
//! An entry's amount, however, must be strictly positive — its sign lives
//! in [`Direction`]. We forward to `Money::positive` so that the rule has
//! exactly one source of truth in the crate.

use crate::{AccountId, Money, MoneyError};

/// Which side of an entry the amount applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Debit side.
    Debit,
    /// Credit side.
    Credit,
}

impl Direction {
    /// The opposite direction. Used when constructing a reversing transaction.
    pub const fn flip(self) -> Self {
        match self {
            Direction::Debit => Direction::Credit,
            Direction::Credit => Direction::Debit,
        }
    }
}

impl core::fmt::Display for Direction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Direction::Debit => "debit",
            Direction::Credit => "credit",
        })
    }
}

/// One side of a posting: an account, a direction, and a positive amount.
///
/// Fields are private. `Entry::new` is the only constructor and enforces
/// the amount-is-positive rule, so it is impossible to hold an `Entry`
/// whose amount is zero or negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    account: AccountId,
    direction: Direction,
    amount: Money,
}

impl Entry {
    /// Construct an entry. The amount must be strictly positive in any
    /// currency; the sign carrying meaning lives in `direction`.
    pub fn new(
        account: AccountId,
        direction: Direction,
        amount: Money,
    ) -> Result<Self, EntryError> {
        // Single source of truth for the "positive money" rule.
        Money::positive(amount.amount(), amount.currency())?;
        Ok(Self {
            account,
            direction,
            amount,
        })
    }

    /// Which account this entry hits.
    pub fn account(&self) -> AccountId {
        self.account
    }

    /// Which side of the entry the amount applies to.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The (positive) amount.
    pub fn amount(&self) -> &Money {
        &self.amount
    }
}

/// Errors raised when constructing an [`Entry`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum EntryError {
    /// Forwarded from `Money::positive`: amount was zero or negative.
    #[error(transparent)]
    Money(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Currency;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn an_account() -> AccountId {
        AccountId::new(Uuid::from_u128(1))
    }

    #[test]
    fn direction_flip_is_involution() {
        assert_eq!(Direction::Debit.flip(), Direction::Credit);
        assert_eq!(Direction::Credit.flip(), Direction::Debit);
        assert_eq!(Direction::Debit.flip().flip(), Direction::Debit);
    }

    #[test]
    fn entry_accepts_positive_amount() {
        let e = Entry::new(
            an_account(),
            Direction::Debit,
            Money::new(dec!(10.00), Currency::Usd),
        )
        .unwrap();
        assert_eq!(e.account(), an_account());
        assert_eq!(e.direction(), Direction::Debit);
        assert_eq!(e.amount().amount(), dec!(10.00));
    }

    #[test]
    fn entry_rejects_zero_amount() {
        let err = Entry::new(
            an_account(),
            Direction::Credit,
            Money::new(dec!(0), Currency::Usd),
        )
        .unwrap_err();
        assert!(matches!(err, EntryError::Money(MoneyError::NonPositive(_))));
    }

    #[test]
    fn entry_rejects_negative_amount() {
        let err = Entry::new(
            an_account(),
            Direction::Credit,
            Money::new(dec!(-0.01), Currency::Usd),
        )
        .unwrap_err();
        assert!(matches!(err, EntryError::Money(MoneyError::NonPositive(_))));
    }

    #[test]
    fn entries_equal_when_fields_equal() {
        // Equality is structural, which the balanced-check and tests both
        // rely on. Pin it down explicitly.
        let a = Entry::new(
            an_account(),
            Direction::Debit,
            Money::new(dec!(1), Currency::Usd),
        )
        .unwrap();
        let b = a.clone();
        assert_eq!(a, b);
    }
}
