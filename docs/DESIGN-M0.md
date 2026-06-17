# DESIGN-M0 — Pure domain core

> Status: **proposed** — awaiting review.
> Scope: M0 only. M1+ are out of scope for this doc.

## 1. Goal of this milestone

Establish the **pure** domain of the ledger, before any database, gRPC, or async
runtime exists. By the end of M0 we will have:

- A Cargo workspace and a hexagonal-architecture skeleton (empty stubs for the
  layers we'll fill in later — *no* code in them beyond what M0 needs).
- A `domain` crate that compiles with **zero IO dependencies** (no `sqlx`, no
  `tonic`, no `tokio`, no `chrono` clock — see §6 on time).
- Core types: `Money`, `Currency`, `AccountId`, `Direction`, `Entry`,
  `Transaction`, `TransactionId`, `IdempotencyKey`.
- The **balanced-transaction check** (INV1) implemented as a pure constructor
  on `Transaction` — i.e. it is impossible to *hold* an unbalanced
  `Transaction` value.
- Unit tests for hand-picked cases, plus `proptest` invariant tests.

Invariants established: **INV1 — Balanced.** No other invariant is in scope yet;
the rest require persistence, concurrency, or a workflow engine.

## 2. Workspace layout

```
ledgercore/
  Cargo.toml              # workspace manifest, [workspace] members + resolver = "2"
  rust-toolchain.toml     # pin stable
  crates/
    domain/               # M0 lives here. Pure, no IO. Tests + proptest inside.
    app/                  # empty stub crate (lib.rs with no public items yet)
    infra-pg/             # empty stub
    infra-chain/          # empty stub
    ledger-svc/           # empty stub (bin? lib? — decided in M2; lib for now)
    settle-svc/           # empty stub
  migrations/             # not created yet (M1)
  proto/                  # not created yet (M2)
  docs/
    DESIGN-M0.md          # this file
```

**Why create the empty stubs now if "don't scaffold ahead" is the rule?**
Tension noted. Two options:

- **(A)** Create only `crates/domain` in M0 and add other crates as their
  milestones arrive. This is the *strictest* reading of "don't scaffold ahead".
- **(B)** Create empty placeholder crates so the workspace shape is visible from
  day one, but write **no code** in them beyond `// placeholder for M{n}`.

I propose **(A)**. It's the more honest reading of the rule, and Cargo
workspaces are trivially extensible — `cargo new --lib crates/app` later is one
command. **Open question for you (Q1 in §10).**

## 3. The domain types

### 3.1 `Currency`

```rust
// Single currency for now (M0 is single-currency by stack rule). Encoded as
// an enum rather than a string so that "USD" vs "usd" cannot drift.
pub enum Currency { USD }
```

**Why an enum, not a `String`?**
- Compile-time guarantees a `Money` cannot be constructed with a typo.
- Mixing-currency bugs become impossible (we cannot even *write* an `Entry`
  whose `Money` is in a different currency to the account, once accounts are
  currency-typed in M1).
- Trade-off: extending currencies needs a code change. Acceptable for a
  single-currency learning project; CLAUDE.md explicitly puts multi-currency
  out of scope.

### 3.2 `Money`

```rust
pub struct Money { amount: Decimal, currency: Currency }
```

Newtype over `rust_decimal::Decimal`, never `f64`.

**Why `Decimal`, not `f64`?**
- Floats cannot represent `0.10` exactly. `0.1 + 0.2 != 0.3`. The accounting
  identity (INV4) demands exact arithmetic. A single rounding error and the
  global debit/credit totals diverge forever.
- `Decimal` is base-10, fixed-point internally, and round-trips through string
  representation.

**Why a newtype around `Decimal`?**
- We control which arithmetic is exposed. The domain only needs `+`, `-`,
  `==`, comparison, negation, and sum — not division by arbitrary scalars.
  Restricting the surface area means a junior reader (me!) can't accidentally
  `Money / 3.7` and lose money to rounding.
- Cross-currency operations become a type error.

**Construction rules.** A `Money` value can be **any** decimal — including
zero and negative — because `Money` represents an *amount* in the abstract.
The constraints "must be positive", "must be non-zero" are properties of
specific use sites:

- `Entry::amount` must be **strictly positive** (the sign is carried by
  `Direction`, never by the amount).
- An account's running balance can be negative (subject to INV5 in M1).

So we will provide:

```rust
impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Self;
    pub fn positive(amount: Decimal, currency: Currency) -> Result<Self, MoneyError>;
}
```

`positive` is what `Entry::new` will call. The plain constructor exists for
balances, sums, and tests.

**Open question Q2 (§10):** do we want to also pin a scale (e.g. 2 dp for USD)
in `Money::new`? My recommendation: **no, not in M0.** Scale is a presentation
concern. The domain enforces *exactness*; truncation/rounding belong at the
API edges. We can add `Money::round_to_currency_scale` later if needed.

### 3.3 `Direction`

```rust
pub enum Direction { Debit, Credit }
```

**Why an explicit enum, not a sign on the amount?**
- Accounting is intrinsically dual-sided. "Debit" and "Credit" are
  *categorical*, not signs — whether a debit *increases* or *decreases* an
  account depends on the account's type (asset vs liability), and we want that
  rule lived in one place, not smuggled into every plus/minus.
- A signed amount invites the bug "I forgot to negate when summing".
- The balanced check becomes more legible: `sum(debits) == sum(credits)`
  rather than `sum(signed_amounts) == 0`. Both are mathematically equivalent;
  the first reads like the textbook definition. (We will implement it the
  signed way internally for elegance — see §4 — but expose the textbook
  framing in error messages.)

### 3.4 `AccountId`, `TransactionId`, `IdempotencyKey`

Newtypes around `Uuid` (for ids) and `String` (for the idempotency key).

**Why newtype IDs?**
- A `fn transfer(from: AccountId, to: AccountId, ...)` can never be called with
  the arguments swapped against a `TransactionId`. Free type-safety.
- Cheap, zero-runtime-cost.

**Why is `IdempotencyKey` in the domain at all when INV6 is M1?**
- INV6 says "submitting the *same* key twice yields one posting". The *check*
  is enforced in persistence (M1). But the *concept* — that a transaction
  carries a caller-supplied dedupe token — belongs to the domain. A
  `Transaction` without an idempotency key is incomplete; making it
  `Option<IdempotencyKey>` in M0 and then mandatory in M1 is just churn.
- We'll validate basic shape (non-empty, max length) in the constructor.

### 3.5 `Entry`

```rust
pub struct Entry {
    pub account: AccountId,
    pub direction: Direction,
    pub amount: Money,  // strictly positive
}
```

Constructor `Entry::new(account, direction, amount)` returns
`Result<Self, EntryError>` and rejects non-positive amounts.

### 3.6 `Transaction`

```rust
pub struct Transaction {
    id: TransactionId,
    idempotency_key: IdempotencyKey,
    entries: Vec<Entry>,        // private — only the constructor can populate
    occurred_at: OccurredAt,    // see §6 on time
}
```

The only public constructor takes a vector of entries and **validates INV1
before returning the value**. After construction the entries field is exposed
read-only. Result: *it is impossible at the type level to hold an unbalanced
`Transaction` in memory.* This is the single most important design move in
M0.

```rust
impl Transaction {
    pub fn new(
        id: TransactionId,
        idempotency_key: IdempotencyKey,
        entries: Vec<Entry>,
        occurred_at: OccurredAt,
    ) -> Result<Self, TransactionError>;
}
```

## 4. The balanced check (INV1) — the conceptual heart of M0

Per CLAUDE.md §4, this is one of the conceptual cores. So we'll **go slow**
and I'll write it out in the doc, then write the actual implementation with
you, line by line.

### Definition (textbook)

> A transaction is balanced iff `Σ(debits) == Σ(credits)`, *currency by
> currency*, and contains at least two entries touching at least two distinct
> accounts.

### Cases the check must catch

1. Empty transaction → reject (`NoEntries`).
2. Single entry → reject (`UnbalancedSingleEntry`) — a transaction needs at
   minimum a debit and a credit.
3. All entries on the same account → reject (`SingleAccount`). Debatable
   whether this is INV1; I argue it's a domain-modelling rule belonging in the
   same constructor. **Open question Q3 (§10).**
4. Mixed currencies → reject (`MixedCurrencies`). Once M0 is single-currency
   this is moot, but the *check* should still exist so the rule is documented
   and the type compiles unchanged in a multi-currency future.
5. `Σ(debits) != Σ(credits)` → reject (`Unbalanced { debits, credits }`).
6. Balanced → accept.

### Implementation sketch

```rust
// Pseudocode — we'll write the real thing together.
fn validate_entries(entries: &[Entry]) -> Result<(), TransactionError> {
    if entries.is_empty() { return Err(NoEntries); }
    if entries.len() < 2  { return Err(UnbalancedSingleEntry); }

    let currency = entries[0].amount.currency();
    if entries.iter().any(|e| e.amount.currency() != currency) {
        return Err(MixedCurrencies);
    }

    let (debits, credits) = entries.iter().fold(
        (Decimal::ZERO, Decimal::ZERO),
        |(d, c), e| match e.direction {
            Direction::Debit  => (d + e.amount.raw(), c),
            Direction::Credit => (d, c + e.amount.raw()),
        },
    );

    if debits != credits {
        return Err(Unbalanced { debits, credits });
    }

    let unique_accounts: HashSet<_> = entries.iter().map(|e| e.account).collect();
    if unique_accounts.len() < 2 { return Err(SingleAccount); }

    Ok(())
}
```

**Things I want to discuss when we implement it together:**
- The `.raw()` accessor on `Money`. Should that even exist? Alternative: implement
  `Sum<Money>` on `Money` and avoid leaking the inner `Decimal`. I prefer the
  alternative; it's stricter and more idiomatic. We can pick when coding.
- Whether to use `try_fold` and short-circuit on currency mismatch in the
  same pass.
- Whether `MixedCurrencies` and `Unbalanced` should be reported together for
  better caller UX. I lean towards "first error wins" in M0 — simpler — and we
  can switch to an aggregating validator later if I'm convinced it matters.

## 5. Errors

`thiserror`-derived `enum TransactionError` and `enum EntryError`,
`enum MoneyError`. No `anyhow` in the domain crate — that's the rule.

```rust
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum TransactionError {
    #[error("transaction has no entries")]
    NoEntries,
    #[error("transaction has only one entry; need at least one debit and one credit")]
    UnbalancedSingleEntry,
    #[error("all entries belong to the same account")]
    SingleAccount,
    #[error("entries use multiple currencies")]
    MixedCurrencies,
    #[error("debits ({debits}) do not equal credits ({credits})")]
    Unbalanced { debits: Decimal, credits: Decimal },
    #[error(transparent)]
    Entry(#[from] EntryError),
}
```

`PartialEq` so that `assert_eq!` in tests reads naturally.

## 6. Time

The domain needs `occurred_at` on a `Transaction`. The pure domain **must not
read the system clock** — that would be IO.

Two options:

- **(A)** Define `OccurredAt(DateTime<Utc>)` in the domain (depending on
  `chrono` for the *type*, not for clock reads). Callers supply the value. The
  `Clock` port lives in the `app` crate (M2+) and is the *only* place a
  wall-clock read happens.
- **(B)** Use a plain `i64` (Unix epoch micros) in the domain to avoid the
  `chrono` dep entirely.

I propose **(A)**. `chrono` (or `time`) as a *type* dependency is fine; it
isn't IO. The compile-time clarity (`DateTime<Utc>` vs an unlabelled `i64`) is
worth it.

**Library choice: `chrono` vs `time`.** `time` is the modern recommendation
and is what `sqlx` uses by default. I propose `time::OffsetDateTime` in UTC.

## 7. Testing strategy

Two layers:

### 7.1 Hand-picked unit tests

For each error variant: a small case that triggers it. Plus a clear positive
case (a two-entry, balanced transaction in USD).

### 7.2 Property tests with `proptest`

Per CLAUDE.md: *"for any randomly generated balanced transaction, the
invariant holds; for any unbalanced one, it is rejected."*

Strategies:

- **`balanced_transaction()`** strategy: generate `n ∈ [2, 10]` entries with
  random debits, then a single credit equal to `Σ(debits)`. Assert
  `Transaction::new(...)` returns `Ok`.
- **`unbalanced_transaction()`** strategy: generate any random vector of
  entries, *filter* out the balanced ones, assert `Transaction::new(...)`
  returns `Err(Unbalanced { .. })`. (Filtering is wasteful — better to
  *construct* unbalanced ones by adding a non-zero delta to a balanced one.
  We'll do the constructive version.)
- **`mixed_currency_transaction()`** — relevant the moment we add a second
  currency; written now, ignored in CI until then.
- **Shrinking matters.** When `proptest` finds a failure it shrinks to the
  minimal counter-example. We must implement `Arbitrary`/strategies that
  shrink towards small `Decimal` values so failures are readable.

**Decimal generation.** `rust_decimal` doesn't have a `proptest` `Arbitrary`
impl. We'll write a small helper: `decimal_in_range(min, max, scale)` that
generates a `Decimal` with a bounded mantissa and fixed scale. Picking the
range matters — too-large values can hide bugs the test means to find, and
overflows are uninteresting noise.

## 8. What we are *not* doing in M0

- No accounts table, no posting, no balances — there is nothing to post to.
- No `Account` *aggregate* with rules (e.g. asset vs liability, allow-negative).
  We only have `AccountId`. Account types live in M1 because they're enforced
  during *posting*, not *transaction construction*.
- No reversing transactions. The mechanism is "post the negation"; it requires
  persistence to be meaningful.
- No IO of any kind. No `tokio`, no `sqlx`, no async.

## 9. Lesson check (for the end of M0)

When M0 is done, you should be able to answer:

1. Why does the domain crate forbid IO dependencies, and what concrete
   benefits does that give us in M1 and M2?
2. Why is `Σ(debits) == Σ(credits)` enforced in `Transaction::new` rather
   than (a) at posting time in M1, or (b) by a separate `validate(&self)`
   method?
3. Why `rust_decimal::Decimal` and not `f64`, in one sentence and one example
   that breaks INV4?

## 10. Open questions for you before I code

- **Q1.** Workspace stubs: option (A) — only create the crates we need
  per milestone — or option (B) — create all stub crates now? I recommend (A).
- **Q2.** Should `Money::new` enforce a currency-specific scale (e.g. ≤ 2 dp
  for USD)? I recommend **no for M0**; it's a presentation concern.
- **Q3.** Should "all entries on one account" be a domain-level rejection
  (`SingleAccount`) or allowed at this layer and rejected higher up? I lean
  reject in the domain.
- **Q4.** `chrono` vs `time` for `OccurredAt`. I recommend `time`.
- **Q5.** Are you OK with `IdempotencyKey` being mandatory on `Transaction`
  in M0, even though the dedupe check itself isn't wired up until M1?
