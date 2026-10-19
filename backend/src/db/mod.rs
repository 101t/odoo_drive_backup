pub mod models;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use models::*;

pub async fn connect(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite://")?
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(15));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .with_context(|| format!("opening database {}", path.display()))?;
    sqlx::migrate!("./migrations").run(&pool).await.context("running database migrations")?;
    Ok(pool)
}

pub async fn get_setting(db: &SqlitePool, key: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(db).await
}

pub async fn set_setting(db: &SqlitePool, key: &str, value: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_instance(db: &SqlitePool, id: i64) -> sqlx::Result<InstanceRow> {
    sqlx::query_as("SELECT * FROM instances WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn get_destination(db: &SqlitePool, id: i64) -> sqlx::Result<DestinationRow> {
    sqlx::query_as("SELECT * FROM destinations WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn get_job(db: &SqlitePool, id: i64) -> sqlx::Result<JobRow> {
    sqlx::query_as("SELECT * FROM jobs WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn job_destination_ids(db: &SqlitePool, job_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar("SELECT destination_id FROM job_destinations WHERE job_id = ? ORDER BY destination_id")
        .bind(job_id)
        .fetch_all(db)
        .await
}

pub async fn get_run(db: &SqlitePool, id: i64) -> sqlx::Result<Run> {
    sqlx::query_as(concat!(crate::run_select!(), " WHERE r.id = ?")).bind(id).fetch_one(db).await
}

pub async fn get_artifact(db: &SqlitePool, id: i64) -> sqlx::Result<Artifact> {
    sqlx::query_as("SELECT * FROM artifacts WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn run_artifacts(db: &SqlitePool, run_id: i64) -> sqlx::Result<Vec<Artifact>> {
    sqlx::query_as("SELECT * FROM artifacts WHERE run_id = ? ORDER BY id").bind(run_id).fetch_all(db).await
}

pub async fn run_logs(db: &SqlitePool, run_id: i64) -> sqlx::Result<Vec<RunLog>> {
    sqlx::query_as("SELECT * FROM run_logs WHERE run_id = ? ORDER BY id").bind(run_id).fetch_all(db).await
}
