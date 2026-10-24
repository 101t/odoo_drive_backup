mod api;
mod auth;
mod config;
mod crypto;
mod db;
mod engine;
mod error;
mod events;
mod i18n;
mod notify;
mod odoo;
mod scheduler;
mod state;
mod storage;
mod util;
mod web;
#[cfg(windows)]
mod winservice;

use std::collections::HashMap;
use std::io::{BufRead, IsTerminal};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::crypto::Crypto;
use crate::db::Db;
use crate::engine::Engine;
use crate::events::EventBus;
use crate::notify::Notifier;
use crate::state::AppState;
use crate::storage::StorageContext;

#[cfg(target_env = "musl")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "odoo-backup", version, about = "Automated backup manager for Odoo instances")]
struct Cli {
    #[command(flatten)]
    config: Config,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the web server and the scheduler (default).
    Serve,
    /// Set a new password for a user (creates the user if it does not exist).
    /// The password is read from standard input.
    ResetPassword { username: String },
    /// Print a new random encryption key suitable for ODB_SECRET_KEY.
    GenerateKey,
    /// Exit 0 when the local server answers /api/health (for container health checks).
    Healthcheck,
}

fn main() -> anyhow::Result<()> {
    // The Windows service manager starts `odoo-backup.exe windows-service <env file>` (see packaging/windows/install.ps1).
    #[cfg(windows)]
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "windows-service") {
        return winservice::run(std::env::args_os().nth(2).map(std::path::PathBuf::from));
    }

    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        // Plain text for journald and other log collectors.
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => runtime.block_on(serve(cli.config, shutdown_signal())),
        Command::ResetPassword { username } => runtime.block_on(reset_password(cli.config, username)),
        Command::GenerateKey => {
            use base64::Engine as _;
            println!("{}", base64::engine::general_purpose::STANDARD.encode(crypto::random_bytes::<32>()));
            Ok(())
        }
        Command::Healthcheck => runtime.block_on(healthcheck(cli.config)),
    }
}

fn log_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info,hyper=warn,reqwest=warn"))
}

async fn healthcheck(config: Config) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.2:{}/api/health", config.bind.port());
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?;
    if !response.status().is_success() {
        bail!("health endpoint returned HTTP {}", response.status());
    }
    Ok(())
}

async fn open_database(config: &Config) -> anyhow::Result<(Db, Arc<Crypto>)> {
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("creating data directory {}", config.data_dir.display()))?;
    let crypto = Arc::new(Crypto::load_or_create(config.secret_key.as_deref(), &config.data_dir)?);
    let pool = db::connect(config.database_url.as_deref(), &config.database_path()).await?;

    // Detect a key that does not match the one used to encrypt the stored secrets.
    match db::get_setting(&pool, "key_check").await? {
        Some(check) => {
            if crypto.decrypt(&check).ok().as_deref() != Some("odoo-backup") {
                bail!(
                    "the encryption key does not match this database. Restore the original secret.key \
                     (or ODB_SECRET_KEY); stored passwords cannot be decrypted otherwise"
                );
            }
        }
        None => db::set_setting(&pool, "key_check", &crypto.encrypt("odoo-backup")).await?,
    }
    Ok((pool, crypto))
}

/// Runs the server until `stop` resolves, then lets running backups wind down.
async fn serve(config: Config, stop: impl Future<Output = ()> + Send + 'static) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let (pool, crypto) = open_database(&config).await?;
    let _claim = pool.claim().await?;
    let temp_dir = config.temp_dir();
    std::fs::create_dir_all(&temp_dir).with_context(|| format!("creating temp directory {}", temp_dir.display()))?;

    bootstrap_admin(&pool, &config).await?;

    let events = EventBus::new();
    let storage = StorageContext::new(
        config.google_api_base.clone(),
        config.google_token_url.clone(),
        config.upload_chunk_bytes(),
    )?;
    let notifier = Notifier::new(pool.clone(), crypto.clone(), config.public_url())?;
    let engine = Engine::new(pool.clone(), config.clone(), crypto.clone(), events.clone(), storage.clone(), notifier);
    engine.recover().await?;

    let state = AppState {
        db: pool.clone(),
        config: config.clone(),
        crypto,
        events,
        engine: engine.clone(),
        storage,
        limiter: Arc::new(auth::LoginLimiter::default()),
        oauth_states: Arc::new(Mutex::new(HashMap::new())),
        scheduler_wake: Arc::new(Notify::new()),
        shutdown: CancellationToken::new(),
        started_at: util::now(),
    };

    let scheduler = scheduler::spawn(state.clone());
    let app = api::router(state.clone());

    let listener =
        tokio::net::TcpListener::bind(config.bind).await.with_context(|| format!("binding {}", config.bind))?;
    tracing::info!(
        address = %config.bind,
        data_dir = %config.data_dir.display(),
        database = pool.engine(),
        version = env!("CARGO_PKG_VERSION"),
        "Odoo Backup is listening"
    );

    let shutdown = state.shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            stop.await;
            tracing::info!("shutting down");
            shutdown.cancel();
        })
        .await?;

    engine.shutdown(Duration::from_secs(20)).await;
    let _ = scheduler.await;
    pool.close().await;
    Ok(())
}

async fn bootstrap_admin(pool: &Db, config: &Config) -> anyhow::Result<()> {
    let (Some(username), Some(password)) = (&config.admin_username, &config.admin_password) else {
        return Ok(());
    };
    let users: i64 = db::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    if users > 0 {
        return Ok(());
    }
    auth::validate_username(username).map_err(|e| anyhow::anyhow!("ODB_ADMIN_USERNAME: {e}"))?;
    auth::validate_password(password).map_err(|e| anyhow::anyhow!("ODB_ADMIN_PASSWORD: {e}"))?;
    let hash = auth::hash_password(password)?;
    db::query("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)")
        .bind(username)
        .bind(hash)
        .bind(util::now())
        .execute(pool)
        .await?;
    tracing::info!(username = %username, "created the initial admin user from the environment");
    Ok(())
}

async fn reset_password(config: Config, username: String) -> anyhow::Result<()> {
    let (pool, _) = open_database(&config).await?;
    auth::validate_username(&username).map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("Enter the new password for \"{username}\" and press Enter:");
    let mut password = String::new();
    std::io::stdin().lock().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']);
    auth::validate_password(password).map_err(|e| anyhow::anyhow!("{e}"))?;
    let hash = auth::hash_password(password)?;
    let updated = db::query("UPDATE users SET password_hash = ? WHERE LOWER(username) = LOWER(?)")
        .bind(&hash)
        .bind(&username)
        .execute(&pool)
        .await?
        .rows_affected();
    if updated == 0 {
        db::query("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)")
            .bind(&username)
            .bind(&hash)
            .bind(util::now())
            .execute(&pool)
            .await?;
        eprintln!("Created user \"{username}\".");
    } else {
        db::query("DELETE FROM sessions WHERE user_id IN (SELECT id FROM users WHERE LOWER(username) = LOWER(?))")
            .bind(&username)
            .execute(&pool)
            .await?;
        eprintln!("Password updated; existing sessions were signed out.");
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
