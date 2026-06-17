//! `recon` — reconciliation CLI. Asserts INV3 (per-account projection
//! consistency) and INV4 (global debits == credits) over live data.
//! See `docs/DESIGN-M1.md` §8. Exits 0 on success, 1 on drift,
//! 2 on operational error.

use std::process::ExitCode;

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::main]
async fn main() -> ExitCode {
    let db_url = match std::env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("DATABASE_URL not set");
            return ExitCode::from(2);
        }
    };

    let pool = match PgPool::connect(&db_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("connect failed: {e}");
            return ExitCode::from(2);
        }
    };

    let inv3 = match per_account_consistency(&pool).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("INV3 query failed: {e}");
            return ExitCode::from(2);
        }
    };
    let (debits, credits) = match global_totals(&pool).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("INV4 query failed: {e}");
            return ExitCode::from(2);
        }
    };

    let inv3_ok = inv3.iter().all(|r| r.stored == r.computed);
    let inv4_ok = debits == credits;

    println!("--- INV3 (per-account: stored balance == Σ entries) ---");
    if inv3.is_empty() {
        println!("  (no accounts)");
    }
    for r in &inv3 {
        let mark = if r.stored == r.computed { "OK " } else { "DRIFT" };
        println!(
            "  {mark}  {id}  stored={stored}  computed={computed}",
            id = r.id,
            stored = r.stored,
            computed = r.computed,
        );
    }

    println!();
    println!("--- INV4 (global: Σ debits == Σ credits) ---");
    println!("  debits  = {debits}");
    println!("  credits = {credits}");
    println!("  {}", if inv4_ok { "OK" } else { "DRIFT" });

    if inv3_ok && inv4_ok {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

struct AccountConsistency {
    id: Uuid,
    stored: Decimal,
    computed: Decimal,
}

async fn per_account_consistency(pool: &PgPool) -> sqlx::Result<Vec<AccountConsistency>> {
    // For each account, recompute its balance from the entries log and
    // compare with the stored value. INV3 fails iff any row differs.
    let rows = sqlx::query!(
        r#"SELECT a.id,
                  a.balance,
                  COALESCE(SUM(
                    CASE
                      WHEN a.kind = 'asset'     AND e.direction = 'debit'  THEN  e.amount
                      WHEN a.kind = 'asset'     AND e.direction = 'credit' THEN -e.amount
                      WHEN a.kind = 'liability' AND e.direction = 'credit' THEN  e.amount
                      WHEN a.kind = 'liability' AND e.direction = 'debit'  THEN -e.amount
                    END
                  ), 0) AS "computed!"
           FROM accounts a
           LEFT JOIN entries e ON e.account_id = a.id
           GROUP BY a.id
           ORDER BY a.id"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AccountConsistency {
            id: r.id,
            stored: r.balance,
            computed: r.computed,
        })
        .collect())
}

async fn global_totals(pool: &PgPool) -> sqlx::Result<(Decimal, Decimal)> {
    // The accounting identity: total debits across the whole ledger
    // must equal total credits. If this drifts, something has bypassed
    // the post_transaction path (or a CHECK constraint is missing).
    let row = sqlx::query!(
        r#"SELECT
              COALESCE(SUM(CASE WHEN direction = 'debit'  THEN amount END), 0) AS "debits!",
              COALESCE(SUM(CASE WHEN direction = 'credit' THEN amount END), 0) AS "credits!"
           FROM entries"#,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.debits, row.credits))
}
