-- M1 dedicated role: INV2 enforced at the database level.
--
-- Roles in Postgres are CLUSTER-global (one cluster can host many DBs;
-- all share the same role catalog). So per-DB test runs must create the
-- role idempotently — the second test's migration would otherwise crash
-- on "role already exists".
--
-- Two roles:
--
--   * The migration role — whoever runs `sqlx migrate run` or psql -f.
--     Has full DDL/DML. Used only for schema changes.
--
--   * `ledgercore_app` (this migration) — NOLOGIN, used as a group.
--     A real application connects with a login role that `INHERIT`s
--     from this one (or uses `SET ROLE ledgercore_app` after connecting
--     as a more-privileged user — what tests do).
--
-- The point: even if someone executes UPDATE entries inside the app,
-- Postgres rejects it because the role has no UPDATE/DELETE privilege.
-- Application discipline can't be the only guarantor of INV2.

DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'ledgercore_app') THEN
        CREATE ROLE ledgercore_app NOLOGIN;
    END IF;
END $$;

-- Schema usage is the prerequisite for any object access.
GRANT USAGE ON SCHEMA public TO ledgercore_app;

-- entries and transactions: SELECT + INSERT only. No UPDATE, no DELETE.
-- This is the load-bearing line for INV2.
GRANT SELECT, INSERT             ON entries      TO ledgercore_app;
GRANT SELECT, INSERT             ON transactions TO ledgercore_app;

-- accounts.balance is the only mutable column anywhere in the schema.
GRANT SELECT, INSERT, UPDATE     ON accounts     TO ledgercore_app;

-- entries.id is BIGSERIAL → backed by an owned sequence. INSERT needs
-- USAGE on the sequence to allocate ids.
GRANT USAGE, SELECT ON SEQUENCE entries_id_seq TO ledgercore_app;
