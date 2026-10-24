use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::valid_database_name;
use crate::db::{self, JobRow, get_instance, get_job, status};
use crate::engine::policy::{self, LatestRun, PolicyScore};
use crate::error::{ApiResult, AppError};
use crate::i18n::Lang;
use crate::odoo::BackupFormat;
use crate::scheduler;
use crate::state::AppState;
use crate::storage::{Capabilities, capabilities};
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
    policy: PolicyScore,
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

/// What the policy score of every job is computed from, loaded with a few queries.
struct PolicyData {
    destinations: HashMap<i64, Vec<i64>>,
    capabilities: HashMap<i64, Capabilities>,
    /// `(database, status, has warning)` of the latest finished backup per database, by job.
    latest: HashMap<i64, Vec<(String, String, bool)>>,
}

impl PolicyData {
    async fn load(state: &AppState) -> sqlx::Result<Self> {
        let pairs: Vec<(i64, i64)> =
            db::query_as("SELECT job_id, destination_id FROM job_destinations ORDER BY job_id, destination_id")
                .fetch_all(&state.db)
                .await?;
        let mut destinations: HashMap<i64, Vec<i64>> = HashMap::new();
        for (job, destination) in pairs {
            destinations.entry(job).or_default().push(destination);
        }
        let rows: Vec<(i64, String, String)> =
            db::query_as("SELECT id, kind, config_json FROM destinations").fetch_all(&state.db).await?;
        let capabilities = rows.into_iter().map(|(id, kind, config)| (id, capabilities(&kind, &config))).collect();
        let runs: Vec<(i64, String, String, bool)> = db::query_as(
            "SELECT job_id, database, status, warning IS NOT NULL FROM runs WHERE id IN ( \
               SELECT MAX(id) FROM runs WHERE job_id IS NOT NULL AND kind = 'backup' AND database != '*' \
               AND status NOT IN ('queued', 'running', 'cancelled') GROUP BY job_id, database)",
        )
        .fetch_all(&state.db)
        .await?;
        let mut latest: HashMap<i64, Vec<(String, String, bool)>> = HashMap::new();
        for (job, database, status, warning) in runs {
            latest.entry(job).or_default().push((database, status, warning));
        }
        Ok(Self { destinations, capabilities, latest })
    }

    fn score(&self, job: &JobRow, destination_ids: &[i64]) -> PolicyScore {
        let caps: Vec<Capabilities> =
            destination_ids.iter().filter_map(|id| self.capabilities.get(id).copied()).collect();
        let covered = job.databases();
        let latest: Vec<LatestRun> = self
            .latest
            .get(&job.id)
            .into_iter()
            .flatten()
            // Databases removed from the job no longer count.
            .filter(|(database, _, _)| job.all_databases || covered.contains(database))
            .map(|(_, status, warning)| LatestRun { status, has_warning: *warning })
            .collect();
        policy::score(&caps, job.verify_archive, &latest)
    }
}

fn to_view(row: JobListRow, data: &PolicyData, lang: Lang) -> JobView {
    let databases = row.job.databases();
    let destination_ids = data.destinations.get(&row.job.id).cloned().unwrap_or_default();
    let policy = data.score(&row.job, &destination_ids);
    let j = row.job;
    JobView {
        id: j.id,
        cron_description: scheduler::describe(&j.cron, lang),
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
        policy,
        created_at: j.created_at,
        updated_at: j.updated_at,
    }
}

async fn load_view(state: &AppState, id: i64, lang: Lang) -> ApiResult<JobView> {
    let row: JobListRow = db::query_as(concat!(job_select!(), " WHERE j.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Job"))?;
    Ok(to_view(row, &PolicyData::load(state).await?, lang))
}

pub async fn list(State(state): State<AppState>, lang: Lang) -> ApiResult<Json<Vec<JobView>>> {
    let rows: Vec<JobListRow> =
        db::query_as(concat!(job_select!(), " ORDER BY LOWER(j.name), j.id")).fetch_all(&state.db).await?;
    let data = PolicyData::load(&state).await?;
    Ok(Json(rows.into_iter().map(|r| to_view(r, &data, lang)).collect()))
}

/// `(job name, policy score)` of every job, for the metrics endpoint.
pub(super) async fn policy_scores(state: &AppState) -> sqlx::Result<Vec<(String, PolicyScore)>> {
    let jobs: Vec<JobRow> = db::query_as("SELECT * FROM jobs ORDER BY id").fetch_all(&state.db).await?;
    let data = PolicyData::load(state).await?;
    Ok(jobs
        .into_iter()
        .map(|job| {
            let ids = data.destinations.get(&job.id).cloned().unwrap_or_default();
            let score = data.score(&job, &ids);
            (job.name, score)
        })
        .collect())
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>, lang: Lang) -> ApiResult<Json<JobView>> {
    Ok(Json(load_view(&state, id, lang).await?))
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
    // Only placeholders are interpolated; the ids are bound.
    let sql = format!("SELECT id FROM destinations WHERE id IN {}", db::placeholders(input.destination_ids.len()));
    let mut query = db::query_scalar::<i64>(sqlx::AssertSqlSafe(sql));
    for id in &input.destination_ids {
        query = query.bind(id);
    }
    let existing = query.fetch_all(&state.db).await?;
    if let Some(id) = input.destination_ids.iter().find(|id| !existing.contains(id)) {
        return Err(AppError::bad_request(format!("Destination #{id} does not exist")));
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

pub async fn create(
    State(state): State<AppState>,
    lang: Lang,
    Json(mut input): Json<JobInput>,
) -> ApiResult<Json<JobView>> {
    validate(&state, &mut input).await?;
    let ts = now();
    let mut tx = state.db.begin().await?;
    let id: i64 = db::query_scalar(
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
    .fetch_one(&mut tx)
    .await?;
    for destination_id in &input.destination_ids {
        db::query("INSERT INTO job_destinations (job_id, destination_id) VALUES (?, ?)")
            .bind(id)
            .bind(destination_id)
            .execute(&mut tx)
            .await?;
    }
    tx.commit().await?;
    state.scheduler_wake.notify_one();
    Ok(Json(load_view(&state, id, lang).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    lang: Lang,
    Json(mut input): Json<JobInput>,
) -> ApiResult<Json<JobView>> {
    get_job(&state.db, id).await?;
    validate(&state, &mut input).await?;
    let mut tx = state.db.begin().await?;
    db::query(
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
    .execute(&mut tx)
    .await?;
    db::query("DELETE FROM job_destinations WHERE job_id = ?").bind(id).execute(&mut tx).await?;
    for destination_id in &input.destination_ids {
        db::query("INSERT INTO job_destinations (job_id, destination_id) VALUES (?, ?)")
            .bind(id)
            .bind(destination_id)
            .execute(&mut tx)
            .await?;
    }
    tx.commit().await?;
    state.scheduler_wake.notify_one();
    Ok(Json(load_view(&state, id, lang).await?))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let active: i64 = db::query_scalar("SELECT COUNT(*) FROM runs WHERE job_id = ? AND status IN (?, ?)")
        .bind(id)
        .bind(status::QUEUED)
        .bind(status::RUNNING)
        .fetch_one(&state.db)
        .await?;
    if active > 0 {
        return Err(AppError::Conflict("This job has runs in progress; wait or cancel them first".into()));
    }
    let deleted = db::query("DELETE FROM jobs WHERE id = ?").bind(id).execute(&state.db).await?;
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

pub async fn cron_preview(lang: Lang, Json(input): Json<CronPreviewInput>) -> Json<Value> {
    match scheduler::upcoming(&input.cron, &input.timezone, Utc::now(), 5) {
        Ok(next) => Json(json!({ "valid": true, "description": scheduler::describe(&input.cron, lang), "next": next })),
        Err(err) => Json(json!({ "valid": false, "error": err.to_string(), "next": [] })),
    }
}
