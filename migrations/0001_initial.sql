-- M1 initial schema.
--
-- Establishes: INV2 (append-only), INV6 (idempotent posting).
-- The non-negative rule (INV5) is enforced at the application layer; the
-- schema only guarantees positive amounts on individual entries.

-- Enums. Adding a value requires a deliberate migration; typos can't insert.
CREATE TYPE currency        AS ENUM ('USD');
CREATE TYPE account_kind    AS ENUM ('asset', 'liability');
CREATE TYPE direction       AS ENUM ('debit', 'credit');

-- Accounts. The only table with a mutable column (balance). In M3 balance
-- becomes a projection derived from an events log; in M1 it's stored.
CREATE TABLE accounts (
    id              UUID            PRIMARY KEY,
    kind            account_kind    NOT NULL,
    currency        currency        NOT NULL,
    allow_negative  BOOLEAN         NOT NULL,
    balance         NUMERIC(38, 9)  NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now()
);

-- Transactions. Append-only header carrying the idempotency key.
CREATE TABLE transactions (
    id              UUID            PRIMARY KEY,
    idempotency_key VARCHAR(128)    NOT NULL,
    occurred_at     TIMESTAMPTZ     NOT NULL,
    posted_at       TIMESTAMPTZ     NOT NULL DEFAULT now()
);

-- INV6: a single row per idempotency key.
CREATE UNIQUE INDEX uq_transactions_idempotency_key
    ON transactions (idempotency_key);

-- Entries. Append-only postings. Positive amount enforced at the DB level
-- as defence in depth (the domain also enforces it via Money::positive).
CREATE TABLE entries (
    id              BIGSERIAL       PRIMARY KEY,
    transaction_id  UUID            NOT NULL REFERENCES transactions(id),
    account_id      UUID            NOT NULL REFERENCES accounts(id),
    direction       direction       NOT NULL,
    amount          NUMERIC(38, 9)  NOT NULL CHECK (amount > 0),
    currency        currency        NOT NULL
);

CREATE INDEX ix_entries_account     ON entries (account_id);
CREATE INDEX ix_entries_transaction ON entries (transaction_id);
