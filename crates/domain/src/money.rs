//! `Money` and `Currency` — exact, currency-aware amounts. See
//! `docs/DESIGN-M0.md` §3 for the design rationale.

use rust_decimal::Decimal;

/// ISO-4217 currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    /// United States dollar.
    Usd,
}

impl Currency {
    /// ISO-4217 alphabetic code.
    pub const fn code(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
        }
    }
}

impl core::fmt::Display for Currency {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

/// A monetary amount in a specific currency.
///
/// May be negative, zero, or positive. Callers that need positivity (e.g.
/// [`Entry`](crate::Entry)) should use [`Money::positive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    amount: Decimal,
    currency: Currency,
}

impl Money {
    /// Construct from any decimal amount.
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Self { amount, currency }
    }

    /// Zero in the given currency. Useful as a fold accumulator.
    pub fn zero(currency: Currency) -> Self {
        Self { amount: Decimal::ZERO, currency }
    }

    /// Construct a strictly positive amount; rejects `<= 0`.
    pub fn positive(amount: Decimal, currency: Currency) -> Result<Self, MoneyError> {
        if amount <= Decimal::ZERO {
            return Err(MoneyError::NonPositive(amount));
        }
        Ok(Self { amount, currency })
    }

    /// The raw `Decimal` amount.
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// The currency.
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Add two amounts; errors if currencies differ.
    pub fn try_add(&self, other: &Money) -> Result<Self, MoneyError> {
        self.require_same_currency(other)?;
        Ok(Self {
            amount: self.amount + other.amount,
            currency: self.currency,
        })
    }

    /// Subtract `other` from `self`; errors if currencies differ. May
    /// produce a negative result.
    pub fn try_sub(&self, other: &Money) -> Result<Self, MoneyError> {
        self.require_same_currency(other)?;
        Ok(Self {
            amount: self.amount - other.amount,
            currency: self.currency,
        })
    }

    fn require_same_currency(&self, other: &Money) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

impl core::fmt::Display for Money {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Preserve scale: "100.00 USD" and "100 USD" convey different precision.
        write!(f, "{} {}", self.amount, self.currency)
    }
}

/// Errors raised when constructing or combining [`Money`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// `Money::positive` received a non-positive value.
    #[error("amount must be strictly positive, got {0}")]
    NonPositive(Decimal),

    /// `try_add` / `try_sub` received two different currencies.
    #[error("currency mismatch: {left} vs {right}")]
    CurrencyMismatch {
        /// Receiver's currency.
        left: Currency,
        /// Argument's currency.
        right: Currency,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn currency_display_is_iso_code() {
        assert_eq!(Currency::Usd.code(), "USD");
        assert_eq!(format!("{}", Currency::Usd), "USD");
    }

    #[test]
    fn money_new_accepts_any_decimal() {
        assert_eq!(Money::new(dec!(0), Currency::Usd).amount(), dec!(0));
        assert_eq!(Money::new(dec!(-12.34), Currency::Usd).amount(), dec!(-12.34));
        assert_eq!(Money::new(dec!(99.99), Currency::Usd).amount(), dec!(99.99));
    }

    #[test]
    fn money_zero_is_zero_in_currency() {
        let z = Money::zero(Currency::Usd);
        assert_eq!(z.amount(), Decimal::ZERO);
        assert_eq!(z.currency(), Currency::Usd);
    }

    #[test]
    fn money_positive_rejects_zero() {
        assert_eq!(
            Money::positive(dec!(0), Currency::Usd),
            Err(MoneyError::NonPositive(dec!(0))),
        );
    }

    #[test]
    fn money_positive_rejects_negative() {
        assert_eq!(
            Money::positive(dec!(-0.01), Currency::Usd),
            Err(MoneyError::NonPositive(dec!(-0.01))),
        );
    }

    #[test]
    fn money_positive_accepts_strictly_positive() {
        assert_eq!(
            Money::positive(dec!(0.01), Currency::Usd).unwrap().amount(),
            dec!(0.01),
        );
    }

    #[test]
    fn try_add_same_currency_is_exact() {
        // The canonical "floats can't do this" example.
        let a = Money::new(dec!(0.1), Currency::Usd);
        let b = Money::new(dec!(0.2), Currency::Usd);
        assert_eq!(a.try_add(&b).unwrap().amount(), dec!(0.3));
    }

    #[test]
    fn try_sub_can_produce_negative() {
        let a = Money::new(dec!(5), Currency::Usd);
        let b = Money::new(dec!(8), Currency::Usd);
        assert_eq!(a.try_sub(&b).unwrap().amount(), dec!(-3));
    }

    #[test]
    fn currency_mismatch_error_equality() {
        // Reachable through the public API only once a second currency exists.
        let err = MoneyError::CurrencyMismatch { left: Currency::Usd, right: Currency::Usd };
        assert_eq!(err.clone(), err);
    }

    #[test]
    fn money_display_preserves_scale() {
        assert_eq!(format!("{}", Money::new(dec!(100.00), Currency::Usd)), "100.00 USD");
    }
}
