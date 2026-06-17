//! `Money` and `Currency` — exact, currency-aware amounts.
//!
//! Design rationale (see also `docs/DESIGN-M0.md` §3.1 / §3.2):
//!
//! - **Why `Decimal`, never `f64`.** Floats cannot represent `0.10` exactly,
//!   so `0.1 + 0.2 != 0.3`. The accounting identity (INV4) demands exact
//!   arithmetic: a single rounding error and global debit/credit totals
//!   diverge forever.
//! - **Why a newtype around `Decimal`.** We control which arithmetic the
//!   domain exposes. Money supports add and subtract; it does **not** expose
//!   multiplication or division, because percentage-style operations belong
//!   at the application edge where rounding rules can be made explicit.
//! - **Why `Currency` is an enum, not a `String`.** A typo (`"USD"` vs
//!   `"usd"`) cannot compile. Adding a currency requires a deliberate code
//!   change. Multi-currency is out of scope for M0 (see CLAUDE.md), so a
//!   single `Usd` variant is honest.
//! - **Why arithmetic is `try_*` instead of `std::ops::Add`.** Different
//!   currencies must not silently combine. `Add` cannot return `Result`,
//!   and panicking on a currency mismatch would hide caller bugs at runtime.
//!   Explicit `try_add` / `try_sub` force the caller to handle the error.

use rust_decimal::Decimal;

/// The ISO-4217 currency of an amount.
///
/// Single variant for M0; the abstraction exists so that
/// every `Money` operation that mixes currencies fails at construction time
/// rather than producing a nonsense sum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    /// United States dollar.
    Usd,
}

impl Currency {
    /// ISO-4217 alphabetic code, e.g. `"USD"`.
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

/// A monetary amount in a specific [`Currency`].
///
/// Values are stored as `rust_decimal::Decimal` and so arithmetic is exact.
///
/// `Money` can be **any** decimal value, including zero or negative. The
/// sign of an amount carries no domain meaning here: balances can be
/// negative (subject to INV5 in M1), and an `Entry` enforces "strictly
/// positive amount, sign carried by `Direction`" via [`Money::positive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    amount: Decimal,
    currency: Currency,
}

impl Money {
    /// Construct a `Money` from any decimal amount.
    ///
    /// Accepts negative, zero, and positive values. Callers that need a
    /// stricter contract (e.g. `Entry`) should use [`Money::positive`].
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Self { amount, currency }
    }

    /// Zero in the given currency. Useful as a fold accumulator.
    pub fn zero(currency: Currency) -> Self {
        Self { amount: Decimal::ZERO, currency }
    }

    /// Construct a strictly-positive `Money`.
    ///
    /// Rejects `amount <= 0` with [`MoneyError::NonPositive`]. This is the
    /// constructor `Entry::new` will call: an entry's amount is always
    /// positive, and the debit/credit sign lives in `Direction`.
    pub fn positive(amount: Decimal, currency: Currency) -> Result<Self, MoneyError> {
        if amount <= Decimal::ZERO {
            return Err(MoneyError::NonPositive(amount));
        }
        Ok(Self { amount, currency })
    }

    /// The raw `Decimal` amount. Use sparingly — prefer arithmetic on
    /// `Money` itself to keep currency-mismatch impossible.
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// The currency this amount is denominated in.
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Add two amounts, requiring the same currency.
    ///
    /// Returns [`MoneyError::CurrencyMismatch`] otherwise.
    pub fn try_add(&self, other: &Money) -> Result<Self, MoneyError> {
        self.require_same_currency(other)?;
        Ok(Self {
            amount: self.amount + other.amount,
            currency: self.currency,
        })
    }

    /// Subtract `other` from `self`, requiring the same currency.
    ///
    /// May produce a negative `Money`; that's intentional — callers (e.g.
    /// balance computation) need negative intermediate values.
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
        // `Decimal`'s Display preserves scale, which we want for ledger
        // legibility: `100.00 USD` should not silently become `100 USD`.
        write!(f, "{} {}", self.amount, self.currency)
    }
}

/// Errors raised when constructing or combining [`Money`] values.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// `Money::positive` was called with a value `<= 0`.
    #[error("amount must be strictly positive, got {0}")]
    NonPositive(Decimal),

    /// `try_add` / `try_sub` was called with two different currencies.
    #[error("currency mismatch: {left} vs {right}")]
    CurrencyMismatch {
        /// The left-hand currency (the receiver).
        left: Currency,
        /// The right-hand currency.
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
        // Plain constructor exists for balances and intermediate sums;
        // it deliberately does not enforce positivity.
        let zero = Money::new(dec!(0), Currency::Usd);
        let negative = Money::new(dec!(-12.34), Currency::Usd);
        let positive = Money::new(dec!(99.99), Currency::Usd);
        assert_eq!(zero.amount(), dec!(0));
        assert_eq!(negative.amount(), dec!(-12.34));
        assert_eq!(positive.amount(), dec!(99.99));
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
        let m = Money::positive(dec!(0.01), Currency::Usd).unwrap();
        assert_eq!(m.amount(), dec!(0.01));
    }

    #[test]
    fn try_add_same_currency_is_exact() {
        // This is the canonical "floats can't do this" example.
        let a = Money::new(dec!(0.1), Currency::Usd);
        let b = Money::new(dec!(0.2), Currency::Usd);
        let sum = a.try_add(&b).unwrap();
        assert_eq!(sum.amount(), dec!(0.3));
    }

    #[test]
    fn try_sub_can_produce_negative() {
        let a = Money::new(dec!(5), Currency::Usd);
        let b = Money::new(dec!(8), Currency::Usd);
        let diff = a.try_sub(&b).unwrap();
        assert_eq!(diff.amount(), dec!(-3));
    }

    // With only one currency variant we can't yet exercise CurrencyMismatch
    // through the public API. The branch is still reachable from internal
    // arithmetic and will be tested the moment a second currency exists.
    // We test the error type's own equality semantics here so the variant
    // itself is at least exercised in M0.
    #[test]
    fn currency_mismatch_error_equality() {
        let err = MoneyError::CurrencyMismatch {
            left: Currency::Usd,
            right: Currency::Usd,
        };
        assert_eq!(err.clone(), err);
    }

    #[test]
    fn money_display_preserves_scale() {
        // Scale-preserving Display matters because "100" and "100.00"
        // convey different precision in a ledger context.
        let m = Money::new(dec!(100.00), Currency::Usd);
        assert_eq!(format!("{m}"), "100.00 USD");
    }
}
