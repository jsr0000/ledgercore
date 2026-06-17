//! `ledger-svc` entry point: wires the Postgres adapter, system clock,
//! UUID generator into a tonic server. See `docs/DESIGN-M2.md` §8.

use std::sync::Arc;

use anyhow::{Context, Result};
use app::LedgerRepo;
use infra_pg::PgLedgerRepo;
use ledger_svc::adapters::{SystemClock, UuidV4Gen};
use ledger_svc::proto::ledger_server::LedgerServer;
use ledger_svc::LedgerService;
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    install_tracing();

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(&db_url)
        .await
        .context("connecting to Postgres")?;

    // Run migrations on startup. Right call for a learning project
    // (cargo run brings up a working server end-to-end); production
    // would run migrations out-of-band so the binary's startup
    // doesn't depend on holding a privileged DB role.
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("running migrations")?;

    let repo: Arc<dyn LedgerRepo> = Arc::new(PgLedgerRepo::new(pool));
    let clock = Arc::new(SystemClock);
    let ids = Arc::new(UuidV4Gen);
    let svc = LedgerService::new(repo, clock, ids);

    let addr = std::env::var("LEDGER_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".into())
        .parse()
        .context("parsing LEDGER_LISTEN_ADDR")?;
    tracing::info!(%addr, "ledger-svc listening");

    Server::builder()
        .add_service(LedgerServer::new(svc))
        .serve_with_shutdown(addr, shutdown_signal())
        .await
        .context("serving")?;

    tracing::info!("ledger-svc shut down cleanly");
    Ok(())
}

fn install_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .json()
        .init();
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        _ = sigint.recv() => tracing::info!("received SIGINT"),
    }
}
