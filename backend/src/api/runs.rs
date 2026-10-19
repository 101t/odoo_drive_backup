use std::collections::HashMap;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{truthy, valid_database_name};
use crate::db::{Artifact, Run, get_artifact, get_destination, get_instance, get_run, run_artifacts, run_logs, status};
use crate::engine::NewBackup;
use crate::error::{ApiResult, AppError};
use crate::events::Progress;
use crate::odoo::BackupFormat;
use crate::state::AppState;
use crate::storage::Storage;
use crate::util::now;

#[derive(Debug, Serialize)]
pub struct RunView {
    #[serde(flatten)]
    run: Run,
    progress: Option<Progress>,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    status: Option<String>,
    instance_id: Option<i64>,
    job_id: Option<i64>,
    kind: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

macro_rules! run_filter {
    () => {
        " WHERE (?1 IS NULL OR r.status = ?1 OR r.status = ?2) \
           AND (?3 IS NULL OR r.instance_id = ?3) \
           AND (?4 IS NULL OR r.job_id = ?4) \
           AND (?5 IS NULL OR r.kind = ?5) \
           AND (?6 IS NULL OR r.database LIKE ?6 OR r.instance_name LIKE ?6 OR r.file_name LIKE ?6)"
    };
}

pub async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let (status_a, status_b) = match q.status.as_deref().filter(|s| !s.is_empty()) {
        Some("active") => (Some(status::QUEUED.to_string()), Some(status::RUNNING.to_string())),
        Some("problems") => (Some(status::FAILED.to_string()), Some(status::PARTIAL.to_string())),
        Some(other) => (Some(other.to_string()), Some(other.to_string())),
        None => (None, None),
    };
    let search =
        q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| format!("%{}%", s.replace('%', "")));
    let kind = q.kind.filter(|k| !k.is_empty());
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);

    let runs: Vec<Run> =
        sqlx::query_as(concat!(crate::run_select!(), run_filter!(), " ORDER BY r.id DESC LIMIT ?7 OFFSET ?8"))
            .bind(&status_a)
            .bind(&status_b)
            .bind(q.instance_id)
            .bind(q.job_id)
            .bind(&kind)
            .bind(&search)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.db)
            .await?;
    let total: i64 = sqlx::query_scalar(concat!("SELECT COUNT(*) FROM runs r", run_filter!()))
        .bind(&status_a)
        .bind(&status_b)
        .bind(q.instance_id)
        .bind(q.job_id)
        .bind(&kind)
        .bind(&search)
        .fetch_one(&state.db)
        .await?;

    let items = with_artifacts(&state, runs).await?;
    Ok(Json(json!({ "items": items, "total": total })))
}

pub(super) async fn with_artifacts(state: &AppState, runs: Vec<Run>) -> ApiResult<Vec<RunView>> {
    let ids: Vec<i64> = runs.iter().map(|r| r.id).collect();
    let artifacts: Vec<Artifact> = if ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as("SELECT * FROM artifacts WHERE run_id IN (SELECT value FROM json_each(?)) ORDER BY id")
            .bind(serde_json::to_string(&ids).map_err(|e| AppError::Internal(e.into()))?)
            .fetch_all(&state.db)
            .await?
    };
    let mut by_run: HashMap<i64, Vec<Artifact>> = HashMap::new();
    for artifact in artifacts {
        by_run.entry(artifact.run_id).or_default().push(artifact);
    }
    Ok(runs
        .into_iter()
        .map(|run| RunView {
            progress: state.engine.progress(run.id),
            artifacts: by_run.remove(&run.id).unwrap_or_default(),
            run,
        })
        .collect())
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let run = get_run(&state.db, id).await?;
    let artifacts = run_artifacts(&state.db, id).await?;
    let logs = run_logs(&state.db, id).await?;
    let restore_options: Option<Value> = run.restore_options.as_deref().and_then(|o| serde_json::from_str(o).ok());
    Ok(Json(json!({
        "run": RunView { progress: state.engine.progress(id), artifacts, run },
        "logs": logs,
        "restore_options": restore_options,
    })))
}

pub async fn cancel(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let run = get_run(&state.db, id).await?;
    if !run.is_active() {
        return Err(AppError::Conflict("This run is not in progress".into()));
    }
    if !state.engine.cancel(id) {
        // Orphaned row (should not happen): close it.
        sqlx::query("UPDATE runs SET status = ?, error = 'Cancelled', finished_at = ? WHERE id = ?")
            .bind(status::CANCELLED)
            .bind(now())
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct RemoveQuery {
    delete_files: Option<String>,
}

pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<RemoveQuery>,
) -> ApiResult<Json<Value>> {
    let run = get_run(&state.db, id).await?;
    if run.is_active() {
        return Err(AppError::Conflict("Cancel the run before deleting it".into()));
    }
    if truthy(q.delete_files.as_deref()) {
        let mut errors = Vec::new();
        for artifact in run_artifacts(&state.db, id).await? {
            if artifact.status == status::ARTIFACT_STORED
                && let Err(err) = delete_stored(&state, &artifact, "manual").await
            {
                errors.push(format!("{}: {err}", artifact.destination_name));
            }
        }
        if !errors.is_empty() {
            return Err(AppError::Conflict(format!(
                "Some files could not be deleted, the run was kept: {}",
                errors.join("; ")
            )));
        }
    }
    sqlx::query("DELETE FROM runs WHERE id = ?").bind(id).execute(&state.db).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_stored(state: &AppState, artifact: &Artifact, reason: &str) -> ApiResult<()> {
    let remote_id = artifact.remote_id.as_deref().ok_or_else(|| AppError::bad_request("No stored file"))?;
    let destination_id =
        artifact.destination_id.ok_or_else(|| AppError::bad_request("The destination of this file was deleted"))?;
    let destination = get_destination(&state.db, destination_id).await?;
    let storage = Storage::open(&destination, &state.crypto, &state.storage)?;
    storage.delete(remote_id).await?;
    sqlx::query("UPDATE artifacts SET status = ?, deleted_at = ?, delete_reason = ? WHERE id = ?")
        .bind(status::ARTIFACT_DELETED)
        .bind(now())
        .bind(reason)
        .bind(artifact.id)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn delete_artifact(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Artifact>> {
    let artifact = get_artifact(&state.db, id).await?;
    if artifact.status != status::ARTIFACT_STORED {
        return Err(AppError::Conflict("This file is not stored".into()));
    }
    delete_stored(&state, &artifact, "manual").await?;
    Ok(Json(get_artifact(&state.db, id).await?))
}

pub async fn download_artifact(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Response> {
    let artifact = get_artifact(&state.db, id).await?;
    if artifact.status != status::ARTIFACT_STORED {
        return Err(AppError::Conflict("This file is not available".into()));
    }
    let run = get_run(&state.db, artifact.run_id).await?;
    let destination_id =
        artifact.destination_id.ok_or_else(|| AppError::bad_request("The destination of this file was deleted"))?;
    let destination = get_destination(&state.db, destination_id).await?;
    let storage = Storage::open(&destination, &state.crypto, &state.storage)?;
    let remote_id = artifact.remote_id.as_deref().unwrap_or_default();
    let (stream, length) = storage.open_stream(remote_id).await?;

    let file_name = run.file_name.unwrap_or_else(|| format!("backup-{}.{}", run.id, run.format));
    let ascii_name: String =
        file_name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{ascii_name}\""))
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(length) = length {
        response = response.header(header::CONTENT_LENGTH, length);
    }
    response.body(Body::from_stream(stream)).map_err(|e| AppError::Internal(e.into())).map(IntoResponse::into_response)
}

#[derive(Debug, Deserialize)]
pub struct BackupNowInput {
    instance_id: i64,
    databases: Vec<String>,
    destination_ids: Vec<i64>,
    #[serde(default = "zip")]
    format: String,
}

fn zip() -> String {
    "zip".into()
}

pub async fn backup_now(State(state): State<AppState>, Json(input): Json<BackupNowInput>) -> ApiResult<Json<Value>> {
    let instance = get_instance(&state.db, input.instance_id).await?;
    if instance.master_password_enc.is_none() {
        return Err(AppError::bad_request("Set the master password of this instance first"));
    }
    let format = BackupFormat::parse(&input.format).ok_or_else(|| AppError::bad_request("Unknown format"))?;
    if input.databases.is_empty() {
        return Err(AppError::bad_request("Select at least one database"));
    }
    if let Some(bad) = input.databases.iter().find(|d| !valid_database_name(d)) {
        return Err(AppError::bad_request(format!("Invalid database name '{bad}'")));
    }
    if input.destination_ids.is_empty() {
        return Err(AppError::bad_request("Select at least one destination"));
    }
    let mut run_ids = Vec::new();
    for database in &input.databases {
        let id = state
            .engine
            .enqueue_backup(NewBackup {
                instance: &instance,
                database,
                format,
                job_id: None,
                destination_ids: &input.destination_ids,
                trigger: "manual",
            })
            .await?;
        run_ids.push(id);
    }
    Ok(Json(json!({ "run_ids": run_ids })))
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct RestoreInput {
    instance_id: i64,
    database: String,
    #[serde(default = "default_true")]
    copy: bool,
    #[serde(default)]
    neutralize: bool,
}

pub async fn restore_artifact(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<RestoreInput>,
) -> ApiResult<Json<Value>> {
    let artifact = get_artifact(&state.db, id).await?;
    let source = get_run(&state.db, artifact.run_id).await?;
    if source.kind != "backup" {
        return Err(AppError::bad_request("Only backups can be restored"));
    }
    let database = input.database.trim();
    if !valid_database_name(database) {
        return Err(AppError::bad_request(
            "Invalid database name: use letters, digits, '_', '-' or '.', starting with a letter or digit",
        ));
    }
    let target = get_instance(&state.db, input.instance_id).await?;
    if target.master_password_enc.is_none() {
        return Err(AppError::bad_request("Set the master password of the target instance first"));
    }
    if target.databases().iter().any(|d| d == database) {
        return Err(AppError::Conflict(format!(
            "A database named \"{database}\" already exists on {}. Restores always create a new database.",
            target.name
        )));
    }
    let run_id = state
        .engine
        .enqueue_restore(&artifact, &source, &target, database, input.copy, input.neutralize)
        .await
        .map_err(|e| AppError::bad_request(format!("{e:#}")))?;
    Ok(Json(json!({ "run_id": run_id })))
}
