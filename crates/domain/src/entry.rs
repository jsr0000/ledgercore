//! `Direction` and `Entry` — the two halves of a posting. See
//! `docs/DESIGN-M0.md` §3.5 for why direction is categorical.

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
    /// The opposite direction. Used when posting a reversing transaction.
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
/// Fields are private; the constructor enforces positivity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    account: AccountId,
    direction: Direction,
    amount: Money,
}

impl Entry {
    /// Construct an entry. The amount must be strictly positive; the
    /// sign is carried by `direction`, not by the amount.
    pub fn new(
        account: AccountId,
        direction: Direction,
        amount: Money,
    ) -> Result<Self, EntryError> {
        // Single source of truth for the "positive money" rule.
        Money::positive(amount.amount(), amount.currency())?;
        Ok(Self { account, direction, amount })
    }

    /// Which account this entry hits.
    pub fn account(&self) -> AccountId {
        self.account
    }

    /// Debit or credit side.
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
    /// Amount was zero or negative (forwarded from `Money::positive`).
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
        let a = Entry::new(
            an_account(),
            Direction::Debit,
            Money::new(dec!(1), Currency::Usd),
        )
        .unwrap();
        assert_eq!(a, a.clone());
    }
}
