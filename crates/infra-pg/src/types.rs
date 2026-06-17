//! Mappings between Postgres enums and the domain enums. Kept narrow:
//! the SQL types live here, conversions in both directions are pure.

use domain::{AccountKind, Currency, Direction};

#[derive(sqlx::Type, Debug, Clone, Copy, PartialEq, Eq)]
#[sqlx(type_name = "account_kind", rename_all = "lowercase")]
pub(crate) enum DbAccountKind {
    Asset,
    Liability,
}

#[derive(sqlx::Type, Debug, Clone, Copy, PartialEq, Eq)]
#[sqlx(type_name = "currency")]
pub(crate) enum DbCurrency {
    #[sqlx(rename = "USD")]
    Usd,
}

#[derive(sqlx::Type, Debug, Clone, Copy, PartialEq, Eq)]
#[sqlx(type_name = "direction", rename_all = "lowercase")]
pub(crate) enum DbDirection {
    Debit,
    Credit,
}

impl From<AccountKind> for DbAccountKind {
    fn from(k: AccountKind) -> Self {
        match k {
            AccountKind::Asset => Self::Asset,
            AccountKind::Liability => Self::Liability,
        }
    }
}

impl From<DbAccountKind> for AccountKind {
    fn from(k: DbAccountKind) -> Self {
        match k {
            DbAccountKind::Asset => Self::Asset,
            DbAccountKind::Liability => Self::Liability,
        }
    }
}

impl From<Currency> for DbCurrency {
    fn from(c: Currency) -> Self {
        match c {
            Currency::Usd => Self::Usd,
        }
    }
}

impl From<DbCurrency> for Currency {
    fn from(c: DbCurrency) -> Self {
        match c {
            DbCurrency::Usd => Self::Usd,
        }
    }
}

impl From<Direction> for DbDirection {
    fn from(d: Direction) -> Self {
        match d {
            Direction::Debit => Self::Debit,
            Direction::Credit => Self::Credit,
        }
    }
}

impl From<DbDirection> for Direction {
    fn from(d: DbDirection) -> Self {
        match d {
            DbDirection::Debit => Self::Debit,
            DbDirection::Credit => Self::Credit,
        }
    }
}
