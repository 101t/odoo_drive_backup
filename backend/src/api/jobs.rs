use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::valid_database_name;
use crate::db::{JobRow, get_instance, get_job, status};
use crate::error::{ApiResult, AppError};
use crate::odoo::BackupFormat;
use crate::scheduler;
use crate::state::AppState;
use crate::util::now;

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct JobInput {
    name: String,
    instance_id: i64,
    #[serde(default)]
    all_databases: bool,
    #[serde(default)]
    databases: Vec<String>,
    format: String,
    cron: String,
    timezone: String,
    #[serde(default = "default_true")]
    enabled: bool,
    keep_last: i64,
    keep_daily: i64,
    keep_weekly: i64,
    keep_monthly: i64,
    keep_yearly: i64,
    retries: i64,
    #[serde(default = "default_true")]
    verify_archive: bool,
    destination_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct JobView {
    id: i64,
    name: String,
    instance_id: i64,
    instance_name: String,
    all_databases: bool,
    databases: Vec<String>,
    format: String,
    cron: String,
    cron_description: String,
    timezone: String,
    enabled: bool,
    keep_last: i64,
    keep_daily: i64,
    keep_weekly: i64,
    keep_monthly: i64,
    keep_yearly: i64,
    retries: i64,
    verify_archive: bool,
    destination_ids: Vec<i64>,
    next_run_at: Option<DateTime<Utc>>,
    last_run_at: Option<DateTime<Utc>>,
    last_status: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct JobListRow {
    #[sqlx(flatten)]
    job: JobRow,
    instance_name: String,
    last_status: Option<String>,
}

macro_rules! job_select {
    () => {
        "SELECT j.*, i.name AS instance_name, \
           (SELECT r.status FROM runs r WHERE r.job_id = j.id AND r.status NOT IN ('queued', 'running') \
              ORDER BY r.id DESC LIMIT 1) AS last_status \
         FROM jobs j JOIN instances i ON i.id = j.instance_id"
    };
}

fn to_view(row: JobListRow, destination_ids: Vec<i64>) -> JobView {
    let databases = row.job.databases();
    let j = row.job;
    JobView {
        id: j.id,
        cron_description: scheduler::describe(&j.cron),
        name: j.name,
        instance_id: j.instance_id,
        instance_name: row.instance_name,
        all_databases: j.all_databases,
        databases,
        format: j.format,
        cron: j.cron,
        timezone: j.timezone,
        enabled: j.enabled,
        keep_last: j.keep_last,
        keep_daily: j.keep_daily,
        keep_weekly: j.keep_weekly,
        keep_monthly: j.keep_monthly,
        keep_yearly: j.keep_yearly,
        retries: j.retries,
        verify_archive: j.verify_archive,
        destination_ids,
        next_run_at: j.next_run_at,
        last_run_at: j.last_run_at,
        last_status: row.last_status,
        created_at: j.created_at,
        updated_at: j.updated_at,
    }
}

async fn destinations_by_job(state: &AppState) -> sqlx::Result<HashMap<i64, Vec<i64>>> {
    let pairs: Vec<(i64, i64)> =
        sqlx::query_as("SELECT job_id, destination_id FROM job_destinations ORDER BY job_id, destination_id")
            .fetch_all(&state.db)
            .await?;
    let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
    for (job, destination) in pairs {
        map.entry(job).or_default().push(destination);
    }
    Ok(map)
}

async fn load_view(state: &AppState, id: i64) -> ApiResult<JobView> {
    let row: JobListRow = sqlx::query_as(concat!(job_select!(), " WHERE j.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Job"))?;
    let mut map = destinations_by_job(state).await?;
    Ok(to_view(row, map.remove(&id).unwrap_or_default()))
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<JobView>>> {
    let rows: Vec<JobListRow> =
        sqlx::query_as(concat!(job_select!(), " ORDER BY j.name COLLATE NOCASE")).fetch_all(&state.db).await?;
    let mut map = destinations_by_job(&state).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let ids = map.remove(&r.job.id).unwrap_or_default();
                to_view(r, ids)
            })
            .collect(),
    ))
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<JobView>> {
    Ok(Json(load_view(&state, id).await?))
}

async fn validate(state: &AppState, input: &mut JobInput) -> ApiResult<()> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() || input.name.len() > 100 {
        return Err(AppError::bad_request("The name is required (max 100 characters)"));
    }
    get_instance(&state.db, input.instance_id)
        .await
        .map_err(|_| AppError::bad_request("The selected instance does not exist"))?;
    if BackupFormat::parse(&input.format).is_none() {
        return Err(AppError::bad_request("The format must be 'zip' or 'dump'"));
    }
    input.cron = input.cron.split_whitespace().collect::<Vec<_>>().join(" ");
    scheduler::parse_cron(&input.cron).map_err(|e| AppError::bad_request(e.to_string()))?;
    scheduler::parse_timezone(&input.timezone).map_err(|e| AppError::bad_request(e.to_string()))?;

    input.databases = input.databases.iter().map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect();
    input.databases.sort();
    input.databases.dedup();
    if !input.all_databases && input.databases.is_empty() {
        return Err(AppError::bad_request("Select at least one database (or all databases)"));
    }
    if let Some(bad) = input.databases.iter().find(|d| !valid_database_name(d)) {
        return Err(AppError::bad_request(format!("Invalid database name '{bad}'")));
    }

    input.destination_ids.sort();
    input.destination_ids.dedup();
    if input.destination_ids.is_empty() {
        return Err(AppError::bad_request("Select at least one destination"));
    }
    for id in &input.destination_ids {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM destinations WHERE id = ?").bind(id).fetch_one(&state.db).await?;
        if exists == 0 {
            return Err(AppError::bad_request(format!("Destination #{id} does not exist")));
        }
    }
    for (label, value) in [
        ("keep_last", input.keep_last),
        ("keep_daily", input.keep_daily),
        ("keep_weekly", input.keep_weekly),
        ("keep_monthly", input.keep_monthly),
        ("keep_yearly", input.keep_yearly),
    ] {
        if !(0..=1000).contains(&value) {
            return Err(AppError::bad_request(format!("{label} must be between 0 and 1000")));
        }
    }
    if !(0..=10).contains(&input.retries) {
        return Err(AppError::bad_request("retries must be between 0 and 10"));
    }
    Ok(())
}

fn next_run_for(input: &JobInput) -> Option<DateTime<Utc>> {
    if input.enabled { scheduler::next_run(&input.cron, &input.timezone, Utc::now()).ok() } else { None }
}

pub async fn create(State(state): State<AppState>, Json(mut input): Json<JobInput>) -> ApiResult<Json<JobView>> {
    validate(&state, &mut input).await?;
    let ts = now();
    let mut tx = state.db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (name, instance_id, all_databases, databases_json, format, cron, timezone, enabled, \
         keep_last, keep_daily, keep_weekly, keep_monthly, keep_yearly, retries, verify_archive, next_run_at, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&input.name)
    .bind(input.instance_id)
    .bind(input.all_databases)
    .bind(serde_json::to_string(&input.databases).map_err(|e| AppError::Internal(e.into()))?)
    .bind(&input.format)
    .bind(&input.cron)
    .bind(&input.timezone)
    .bind(input.enabled)
    .bind(input.keep_last)
    .bind(input.keep_daily)
    .bind(input.keep_weekly)
    .bind(input.keep_monthly)
    .bind(input.keep_yearly)
    .bind(input.retries)
    .bind(input.verify_archive)
    .bind(next_run_for(&input))
    .bind(ts)
    .bind(ts)
    .fetch_one(&mut *tx)
    .await?;
    for destination_id in &input.destination_ids {
        sqlx::query("INSERT INTO job_destinations (job_id, destination_id) VALUES (?, ?)")
            .bind(id)
            .bind(destination_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    state.scheduler_wake.notify_one();
    Ok(Json(load_view(&state, id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(mut input): Json<JobInput>,
) -> ApiResult<Json<JobView>> {
    get_job(&state.db, id).await?;
    validate(&state, &mut input).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE jobs SET name = ?, instance_id = ?, all_databases = ?, databases_json = ?, format = ?, cron = ?, \
         timezone = ?, enabled = ?, keep_last = ?, keep_daily = ?, keep_weekly = ?, keep_monthly = ?, \
         keep_yearly = ?, retries = ?, verify_archive = ?, next_run_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&input.name)
    .bind(input.instance_id)
    .bind(input.all_databases)
    .bind(serde_json::to_string(&input.databases).map_err(|e| AppError::Internal(e.into()))?)
    .bind(&input.format)
    .bind(&input.cron)
    .bind(&input.timezone)
    .bind(input.enabled)
    .bind(input.keep_last)
    .bind(input.keep_daily)
    .bind(input.keep_weekly)
    .bind(input.keep_monthly)
    .bind(input.keep_yearly)
    .bind(input.retries)
    .bind(input.verify_archive)
    .bind(next_run_for(&input))
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM job_destinations WHERE job_id = ?").bind(id).execute(&mut *tx).await?;
    for destination_id in &input.destination_ids {
        sqlx::query("INSERT INTO job_destinations (job_id, destination_id) VALUES (?, ?)")
            .bind(id)
            .bind(destination_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    state.scheduler_wake.notify_one();
    Ok(Json(load_view(&state, id).await?))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE job_id = ? AND status IN (?, ?)")
        .bind(id)
        .bind(status::QUEUED)
        .bind(status::RUNNING)
        .fetch_one(&state.db)
        .await?;
    if active > 0 {
        return Err(AppError::Conflict("This job has runs in progress; wait or cancel them first".into()));
    }
    let deleted = sqlx::query("DELETE FROM jobs WHERE id = ?").bind(id).execute(&state.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::not_found("Job"));
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn run_now(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    get_job(&state.db, id).await?;
    let run_ids = state.engine.trigger_job(id, "manual").await.map_err(|e| AppError::bad_request(format!("{e:#}")))?;
    if run_ids.is_empty() {
        return Err(AppError::Conflict(
            "Nothing was started: every database of this job is already being backed up".into(),
        ));
    }
    Ok(Json(json!({ "run_ids": run_ids })))
}

#[derive(Debug, Deserialize)]
pub struct CronPreviewInput {
    cron: String,
    #[serde(default = "utc")]
    timezone: String,
}

fn utc() -> String {
    "UTC".into()
}

pub async fn cron_preview(Json(input): Json<CronPreviewInput>) -> Json<Value> {
    match scheduler::upcoming(&input.cron, &input.timezone, Utc::now(), 5) {
        Ok(next) => Json(json!({ "valid": true, "description": scheduler::describe(&input.cron), "next": next })),
        Err(err) => Json(json!({ "valid": false, "error": err.to_string(), "next": [] })),
    }
}
