use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{InstanceRow, get_instance, status};
use crate::error::{ApiResult, AppError};
use crate::odoo::{OdooClient, OdooError, normalize_url};
use crate::state::AppState;
use crate::util::now;

const REFRESH_ON_SAVE_TIMEOUT: Duration = Duration::from_secs(12);

fn default_true() -> bool {
    true
}
fn default_timeout() -> i64 {
    3600
}

#[derive(Debug, Deserialize)]
pub struct InstanceInput {
    name: String,
    url: String,
    /// Empty or absent on update = keep the stored password.
    #[serde(default)]
    master_password: Option<String>,
    #[serde(default = "default_true")]
    verify_tls: bool,
    #[serde(default = "default_timeout")]
    timeout_secs: i64,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Serialize)]
pub struct InstanceView {
    id: i64,
    name: String,
    url: String,
    has_master_password: bool,
    verify_tls: bool,
    timeout_secs: i64,
    notes: String,
    odoo_version: Option<String>,
    databases: Vec<String>,
    last_checked_at: Option<DateTime<Utc>>,
    last_check_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    job_count: i64,
    last_backup_at: Option<DateTime<Utc>>,
    last_backup_status: Option<String>,
}

#[derive(sqlx::FromRow)]
struct InstanceListRow {
    #[sqlx(flatten)]
    instance: InstanceRow,
    job_count: i64,
    last_backup_at: Option<DateTime<Utc>>,
    last_backup_status: Option<String>,
}

impl From<InstanceListRow> for InstanceView {
    fn from(row: InstanceListRow) -> Self {
        let databases = row.instance.databases();
        let i = row.instance;
        Self {
            id: i.id,
            name: i.name,
            url: i.url,
            has_master_password: i.master_password_enc.is_some(),
            verify_tls: i.verify_tls,
            timeout_secs: i.timeout_secs,
            notes: i.notes,
            odoo_version: i.odoo_version,
            databases,
            last_checked_at: i.last_checked_at,
            last_check_error: i.last_check_error,
            created_at: i.created_at,
            updated_at: i.updated_at,
            job_count: row.job_count,
            last_backup_at: row.last_backup_at,
            last_backup_status: row.last_backup_status,
        }
    }
}

macro_rules! instance_select {
    () => {
        "SELECT i.*, \
           (SELECT COUNT(*) FROM jobs j WHERE j.instance_id = i.id) AS job_count, \
           (SELECT MAX(r.finished_at) FROM runs r WHERE r.instance_id = i.id AND r.kind = 'backup' \
              AND r.status IN ('success', 'partial')) AS last_backup_at, \
           (SELECT r.status FROM runs r WHERE r.instance_id = i.id AND r.kind = 'backup' \
              AND r.status NOT IN ('queued', 'running') ORDER BY r.id DESC LIMIT 1) AS last_backup_status \
         FROM instances i"
    };
}

async fn load_view(state: &AppState, id: i64) -> ApiResult<InstanceView> {
    let row: InstanceListRow = sqlx::query_as(concat!(instance_select!(), " WHERE i.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Instance"))?;
    Ok(row.into())
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<InstanceView>>> {
    let rows: Vec<InstanceListRow> =
        sqlx::query_as(concat!(instance_select!(), " ORDER BY i.name COLLATE NOCASE")).fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct DatabaseStat {
    database: String,
    last_success_at: Option<DateTime<Utc>>,
    last_run_at: Option<DateTime<Utc>>,
    runs: i64,
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let view = load_view(&state, id).await?;
    let stats: Vec<DatabaseStat> = sqlx::query_as(
        "SELECT database, \
           MAX(CASE WHEN status IN ('success', 'partial') THEN finished_at END) AS last_success_at, \
           MAX(created_at) AS last_run_at, COUNT(*) AS runs \
         FROM runs WHERE instance_id = ? AND kind = 'backup' AND database != '*' GROUP BY database",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let last: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT database, status, size_bytes FROM runs WHERE id IN ( \
           SELECT MAX(id) FROM runs WHERE instance_id = ? AND kind = 'backup' \
           AND status NOT IN ('queued', 'running') GROUP BY database)",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let jobs: Vec<(i64, String, bool, String, bool)> =
        sqlx::query_as("SELECT id, name, all_databases, databases_json, enabled FROM jobs WHERE instance_id = ?")
            .bind(id)
            .fetch_all(&state.db)
            .await?;

    let mut names: BTreeSet<String> = view.databases.iter().cloned().collect();
    names.extend(stats.iter().map(|s| s.database.clone()));
    let stats: BTreeMap<String, DatabaseStat> = stats.into_iter().map(|s| (s.database.clone(), s)).collect();
    let last: BTreeMap<String, (String, Option<i64>)> =
        last.into_iter().map(|(db, st, size)| (db, (st, size))).collect();

    let databases: Vec<Value> = names
        .iter()
        .map(|name| {
            let stat = stats.get(name);
            let covering: Vec<Value> = jobs
                .iter()
                .filter(|(_, _, all, list, _)| {
                    *all || serde_json::from_str::<Vec<String>>(list).unwrap_or_default().contains(name)
                })
                .map(|(job_id, job_name, _, _, enabled)| json!({ "id": job_id, "name": job_name, "enabled": enabled }))
                .collect();
            json!({
                "name": name,
                "exists": view.databases.contains(name),
                "last_success_at": stat.and_then(|s| s.last_success_at),
                "last_run_at": stat.and_then(|s| s.last_run_at),
                "runs": stat.map(|s| s.runs).unwrap_or(0),
                "last_status": last.get(name).map(|(s, _)| s.clone()),
                "last_size": last.get(name).and_then(|(_, size)| *size),
                "jobs": covering,
            })
        })
        .collect();

    let mut value = serde_json::to_value(view).map_err(|e| AppError::Internal(e.into()))?;
    value["database_details"] = Value::Array(databases);
    Ok(Json(value))
}

fn validate(input: &InstanceInput) -> ApiResult<String> {
    if input.name.trim().is_empty() || input.name.len() > 100 {
        return Err(AppError::bad_request("The name is required (max 100 characters)"));
    }
    if !(60..=86_400).contains(&input.timeout_secs) {
        return Err(AppError::bad_request("The timeout must be between 60 and 86400 seconds"));
    }
    normalize_url(&input.url).map_err(AppError::BadRequest)
}

pub async fn create(State(state): State<AppState>, Json(input): Json<InstanceInput>) -> ApiResult<Json<InstanceView>> {
    let url = validate(&input)?;
    let password = input.master_password.as_deref().filter(|p| !p.is_empty()).map(|p| state.crypto.encrypt(p));
    let ts = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO instances (name, url, master_password_enc, verify_tls, timeout_secs, notes, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&url)
    .bind(password)
    .bind(input.verify_tls)
    .bind(input.timeout_secs)
    .bind(input.notes.trim())
    .bind(ts)
    .bind(ts)
    .fetch_one(&state.db)
    .await?;
    refresh_after_save(&state, id).await;
    Ok(Json(load_view(&state, id).await?))
}

/// Fetches version and databases right away, without letting an unreachable server block the save.
async fn refresh_after_save(state: &AppState, id: i64) {
    if tokio::time::timeout(REFRESH_ON_SAVE_TIMEOUT, refresh_cache(state, id)).await.is_err() {
        let _ = sqlx::query("UPDATE instances SET last_checked_at = ?, last_check_error = ? WHERE id = ?")
            .bind(now())
            .bind("Saved, but the server did not answer in time. Use Refresh to retry.")
            .bind(id)
            .execute(&state.db)
            .await;
    }
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<InstanceInput>,
) -> ApiResult<Json<InstanceView>> {
    let url = validate(&input)?;
    let existing = get_instance(&state.db, id).await?;
    let password = match input.master_password.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => Some(state.crypto.encrypt(p)),
        None => existing.master_password_enc,
    };
    sqlx::query(
        "UPDATE instances SET name = ?, url = ?, master_password_enc = ?, verify_tls = ?, timeout_secs = ?, \
         notes = ?, updated_at = ? WHERE id = ?",
    )
    .bind(input.name.trim())
    .bind(&url)
    .bind(password)
    .bind(input.verify_tls)
    .bind(input.timeout_secs)
    .bind(input.notes.trim())
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    refresh_after_save(&state, id).await;
    Ok(Json(load_view(&state, id).await?))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE instance_id = ? AND status IN (?, ?)")
        .bind(id)
        .bind(status::QUEUED)
        .bind(status::RUNNING)
        .fetch_one(&state.db)
        .await?;
    if active > 0 {
        return Err(AppError::Conflict("This instance has runs in progress; wait or cancel them first".into()));
    }
    let deleted = sqlx::query("DELETE FROM instances WHERE id = ?").bind(id).execute(&state.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::not_found("Instance"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct TestInput {
    url: String,
    #[serde(default)]
    master_password: Option<String>,
    #[serde(default = "default_true")]
    verify_tls: bool,
    /// Use the stored password of this instance when `master_password` is empty.
    #[serde(default)]
    instance_id: Option<i64>,
}

#[derive(Debug, Serialize, Default)]
pub struct TestResult {
    ok: bool,
    url: Option<String>,
    version: Option<String>,
    databases: Vec<String>,
    list_error: Option<String>,
    /// `valid`, `invalid`, `unknown` or `missing`.
    master_password: &'static str,
    error: Option<String>,
}

async fn run_test(url: &str, verify_tls: bool, password: Option<&str>) -> TestResult {
    let mut result = TestResult { master_password: "missing", ..Default::default() };
    let client = match OdooClient::new(url, verify_tls, Duration::from_secs(30)) {
        Ok(c) => c,
        Err(e) => {
            result.error = Some(e.to_string());
            return result;
        }
    };
    result.url = Some(client.base_url().to_string());
    match client.version().await {
        Ok(v) => result.version = Some(v.server_version),
        Err(e) => {
            result.error = Some(match e {
                OdooError::Transport(msg) => msg,
                other => format!("This does not look like a reachable Odoo server: {other}"),
            });
            return result;
        }
    }
    match client.list_databases().await {
        Ok(list) => result.databases = list,
        Err(e) => result.list_error = Some(e.to_string()),
    }
    if let Some(password) = password {
        result.master_password = match client.check_master_password(password).await {
            Ok(Some(true)) => "valid",
            Ok(Some(false)) => "invalid",
            _ => "unknown",
        };
    }
    result.ok = result.master_password != "invalid";
    result
}

pub async fn test_connection(
    State(state): State<AppState>,
    Json(input): Json<TestInput>,
) -> ApiResult<Json<TestResult>> {
    let password = match input.master_password.filter(|p| !p.is_empty()) {
        Some(p) => Some(p),
        None => match input.instance_id {
            Some(id) => get_instance(&state.db, id)
                .await?
                .master_password_enc
                .map(|enc| state.crypto.decrypt(&enc))
                .transpose()?,
            None => None,
        },
    };
    Ok(Json(run_test(&input.url, input.verify_tls, password.as_deref()).await))
}

/// Re-checks an instance and caches its version and database list.
pub async fn refresh(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let result = refresh_cache(&state, id).await?;
    let view = load_view(&state, id).await?;
    Ok(Json(json!({ "instance": view, "test": result })))
}

async fn refresh_cache(state: &AppState, id: i64) -> ApiResult<TestResult> {
    let instance = get_instance(&state.db, id).await?;
    let password = instance.master_password_enc.as_deref().map(|e| state.crypto.decrypt(e)).transpose()?;
    let result = run_test(&instance.url, instance.verify_tls, password.as_deref()).await;
    let error = result.error.clone().or_else(|| {
        (result.master_password == "invalid").then(|| "The master password is rejected by Odoo".to_string())
    });
    if result.error.is_none() {
        let databases = if result.list_error.is_none() {
            serde_json::to_string(&result.databases).unwrap_or_else(|_| "[]".into())
        } else {
            instance.databases_json.clone()
        };
        sqlx::query(
            "UPDATE instances SET odoo_version = ?, databases_json = ?, last_checked_at = ?, last_check_error = ? \
             WHERE id = ?",
        )
        .bind(&result.version)
        .bind(databases)
        .bind(now())
        .bind(error.or_else(|| result.list_error.clone()))
        .bind(id)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query("UPDATE instances SET last_checked_at = ?, last_check_error = ? WHERE id = ?")
            .bind(now())
            .bind(error)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(result)
}
