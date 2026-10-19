use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use super::runs::with_artifacts;
use crate::crypto::constant_time_eq;
use crate::db::{self, Run};
use crate::error::ApiResult;
use crate::state::AppState;

/// (last successful backup, last run) of a database.
type RunTimes = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseHealth {
    instance_id: i64,
    instance_name: String,
    database: String,
    last_status: Option<String>,
    last_success_at: Option<DateTime<Utc>>,
    last_run_at: Option<DateTime<Utc>>,
    last_size: Option<i64>,
    protected: bool,
    stale: bool,
}

/// Backup health of every known database (from instances, jobs and history).
pub async fn database_health(state: &AppState, stale_hours: i64) -> sqlx::Result<Vec<DatabaseHealth>> {
    let instances: Vec<(i64, String, String)> =
        db::query_as("SELECT id, name, databases_json FROM instances").fetch_all(&state.db).await?;
    let jobs: Vec<(i64, bool, String)> =
        db::query_as("SELECT instance_id, all_databases, databases_json FROM jobs WHERE enabled = TRUE")
            .fetch_all(&state.db)
            .await?;
    #[allow(clippy::type_complexity)]
    let stats: Vec<(i64, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> = db::query_as(
        "SELECT instance_id, database, \
           MAX(CASE WHEN status IN ('success', 'partial') THEN finished_at END), MAX(created_at) \
         FROM runs WHERE kind = 'backup' AND instance_id IS NOT NULL AND database != '*' \
         GROUP BY instance_id, database",
    )
    .fetch_all(&state.db)
    .await?;
    let last: Vec<(i64, String, String, Option<i64>)> = db::query_as(
        "SELECT instance_id, database, status, size_bytes FROM runs WHERE id IN ( \
           SELECT MAX(id) FROM runs WHERE kind = 'backup' AND instance_id IS NOT NULL AND database != '*' \
           AND status NOT IN ('queued', 'running') GROUP BY instance_id, database)",
    )
    .fetch_all(&state.db)
    .await?;
    let last_success_size: Vec<(i64, String, Option<i64>)> = db::query_as(
        "SELECT instance_id, database, size_bytes FROM runs WHERE id IN ( \
           SELECT MAX(id) FROM runs WHERE kind = 'backup' AND instance_id IS NOT NULL \
           AND status IN ('success', 'partial') GROUP BY instance_id, database)",
    )
    .fetch_all(&state.db)
    .await?;

    let names: BTreeMap<i64, String> = instances.iter().map(|(id, name, _)| (*id, name.clone())).collect();
    // Databases currently present on their server.
    let mut existing: BTreeSet<(i64, String)> = BTreeSet::new();
    for (id, _, list) in &instances {
        for db in serde_json::from_str::<Vec<String>>(list).unwrap_or_default() {
            existing.insert((*id, db));
        }
    }
    let mut keys = existing.clone();
    let mut all_db_instances = BTreeSet::new();
    let mut protected: BTreeSet<(i64, String)> = BTreeSet::new();
    for (instance_id, all, list) in &jobs {
        if *all {
            all_db_instances.insert(*instance_id);
        }
        for db in serde_json::from_str::<Vec<String>>(list).unwrap_or_default() {
            keys.insert((*instance_id, db.clone()));
            protected.insert((*instance_id, db));
        }
    }
    let stats: BTreeMap<(i64, String), RunTimes> = stats.into_iter().map(|(i, d, s, r)| ((i, d), (s, r))).collect();
    keys.extend(stats.keys().cloned());
    let last: BTreeMap<(i64, String), (String, Option<i64>)> =
        last.into_iter().map(|(i, d, s, size)| ((i, d), (s, size))).collect();
    let success_size: BTreeMap<(i64, String), Option<i64>> =
        last_success_size.into_iter().map(|(i, d, s)| ((i, d), s)).collect();

    let threshold = Utc::now() - Duration::hours(stale_hours);
    let mut out: Vec<DatabaseHealth> = keys
        .into_iter()
        .filter_map(|key| {
            let instance_name = names.get(&key.0)?.clone();
            let (last_success_at, last_run_at) = stats.get(&key).cloned().unwrap_or((None, None));
            let is_protected = protected.contains(&key) || all_db_instances.contains(&key.0);
            // One-off runs against databases that no longer exist are history, not health.
            if !is_protected && !existing.contains(&key) {
                return None;
            }
            Some(DatabaseHealth {
                instance_id: key.0,
                instance_name,
                last_status: last.get(&key).map(|(s, _)| s.clone()),
                last_size: success_size.get(&key).cloned().flatten(),
                stale: is_protected && last_success_at.is_none_or(|t| t < threshold),
                protected: is_protected,
                last_success_at,
                last_run_at,
                database: key.1,
            })
        })
        .collect();
    let rank = |h: &DatabaseHealth| match (h.last_status.as_deref(), h.stale, h.protected) {
        (Some("failed"), _, _) => 0,
        (Some("partial"), _, _) | (_, true, _) => 1,
        (_, _, false) => 3,
        _ => 2,
    };
    out.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.instance_name.cmp(&b.instance_name))
            .then_with(|| a.database.cmp(&b.database))
    });
    Ok(out)
}

pub async fn overview(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let settings = super::settings::load(&state).await?;
    let health = database_health(&state, settings.stale_after_hours).await?;

    let (instances, jobs, jobs_enabled, destinations): (i64, i64, i64, i64) = db::query_as(
        "SELECT (SELECT COUNT(*) FROM instances), (SELECT COUNT(*) FROM jobs), \
                (SELECT COUNT(*) FROM jobs WHERE enabled = TRUE), (SELECT COUNT(*) FROM destinations)",
    )
    .fetch_one(&state.db)
    .await?;

    let since = Utc::now() - Duration::hours(24);
    let (success, partial, failed, bytes): (i64, i64, i64, i64) = db::query_as(
        "SELECT COUNT(*) FILTER (WHERE status = 'success'), COUNT(*) FILTER (WHERE status = 'partial'), \
                COUNT(*) FILTER (WHERE status = 'failed'), \
                CAST(COALESCE(SUM(size_bytes) FILTER (WHERE status IN ('success', 'partial')), 0) AS BIGINT) \
         FROM runs WHERE kind = 'backup' AND created_at >= ?",
    )
    .bind(since)
    .fetch_one(&state.db)
    .await?;

    let storage: Vec<(i64, String, String, i64, i64)> = db::query_as(
        "SELECT d.id, d.name, d.kind, COUNT(a.id), CAST(COALESCE(SUM(a.size_bytes), 0) AS BIGINT) \
         FROM destinations d \
         LEFT JOIN artifacts a ON a.destination_id = d.id AND a.status = 'stored' \
         GROUP BY d.id, d.name, d.kind ORDER BY LOWER(d.name), d.id",
    )
    .fetch_all(&state.db)
    .await?;

    // Grouped per UTC day here rather than in SQL: day arithmetic differs between engines.
    let today = Utc::now().date_naive();
    let first_day = today - Duration::days(13);
    let activity: Vec<(DateTime<Utc>, String, Option<i64>)> =
        db::query_as("SELECT created_at, status, size_bytes FROM runs WHERE kind = 'backup' AND created_at >= ?")
            .bind(first_day.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())
            .fetch_all(&state.db)
            .await?;
    let mut by_day: BTreeMap<NaiveDate, (i64, i64, i64)> = BTreeMap::new();
    for (created_at, status, size) in activity {
        let entry = by_day.entry(created_at.date_naive()).or_default();
        match status.as_str() {
            "success" => entry.0 += 1,
            "failed" | "partial" => entry.1 += 1,
            _ => {}
        }
        if status == "success" || status == "partial" {
            entry.2 += size.unwrap_or(0);
        }
    }
    let days: Vec<Value> = first_day
        .iter_days()
        .take(14)
        .map(|day| {
            let (s, f, b) = by_day.get(&day).cloned().unwrap_or_default();
            json!({ "day": day.format("%Y-%m-%d").to_string(), "success": s, "failed": f, "bytes": b })
        })
        .collect();

    let upcoming: Vec<(i64, String, String, Option<DateTime<Utc>>)> = db::query_as(
        "SELECT j.id, j.name, i.name, j.next_run_at FROM jobs j JOIN instances i ON i.id = j.instance_id \
         WHERE j.enabled = TRUE AND j.next_run_at IS NOT NULL ORDER BY j.next_run_at LIMIT 6",
    )
    .fetch_all(&state.db)
    .await?;

    let active_runs: Vec<Run> =
        db::query_as(concat!(crate::run_select!(), " WHERE r.status IN ('queued', 'running') ORDER BY r.id"))
            .fetch_all(&state.db)
            .await?;
    let recent_runs: Vec<Run> = db::query_as(concat!(
        crate::run_select!(),
        " WHERE r.status NOT IN ('queued', 'running') ORDER BY r.id DESC LIMIT 8"
    ))
    .fetch_all(&state.db)
    .await?;

    let protected = health.iter().filter(|h| h.protected).count();
    let stale = health.iter().filter(|h| h.stale).count();
    let failing = health.iter().filter(|h| h.last_status.as_deref() == Some("failed")).count();

    Ok(Json(json!({
        "counts": {
            "instances": instances,
            "databases": health.len(),
            "protected": protected,
            "stale": stale,
            "failing": failing,
            "jobs": jobs,
            "jobs_enabled": jobs_enabled,
            "destinations": destinations,
        },
        "last24h": { "success": success, "partial": partial, "failed": failed, "bytes": bytes },
        "storage": storage.into_iter().map(|(id, name, kind, files, bytes)| json!({
            "id": id, "name": name, "kind": kind, "files": files, "bytes": bytes,
        })).collect::<Vec<_>>(),
        "activity": days,
        "upcoming": upcoming.into_iter().map(|(id, name, instance, next)| json!({
            "job_id": id, "job_name": name, "instance_name": instance, "next_run_at": next,
        })).collect::<Vec<_>>(),
        "health": health,
        "active": with_artifacts(&state, active_runs).await?,
        "recent": with_artifacts(&state, recent_runs).await?,
        "stale_after_hours": settings.stale_after_hours,
    })))
}

fn label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Prometheus exposition. Enabled only when `ODB_METRICS_TOKEN` is set.
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let Some(expected) = state.config.metrics_token.as_deref() else {
        return Ok((StatusCode::NOT_FOUND, "metrics are disabled (set ODB_METRICS_TOKEN)").into_response());
    };
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        return Ok((StatusCode::UNAUTHORIZED, "invalid token").into_response());
    }

    let settings = super::settings::load(&state).await?;
    let health = database_health(&state, settings.stale_after_hours).await?;
    let mut out = String::new();
    let _ = writeln!(out, "# HELP odoo_backup_last_success_timestamp_seconds Time of the last successful backup.");
    let _ = writeln!(out, "# TYPE odoo_backup_last_success_timestamp_seconds gauge");
    for h in &health {
        if let Some(ts) = h.last_success_at {
            let _ = writeln!(
                out,
                "odoo_backup_last_success_timestamp_seconds{{instance=\"{}\",database=\"{}\"}} {}",
                label(&h.instance_name),
                label(&h.database),
                ts.timestamp()
            );
        }
    }
    let _ = writeln!(out, "# HELP odoo_backup_last_size_bytes Size of the last successful backup.");
    let _ = writeln!(out, "# TYPE odoo_backup_last_size_bytes gauge");
    for h in &health {
        if let Some(size) = h.last_size {
            let _ = writeln!(
                out,
                "odoo_backup_last_size_bytes{{instance=\"{}\",database=\"{}\"}} {size}",
                label(&h.instance_name),
                label(&h.database)
            );
        }
    }
    let _ = writeln!(out, "# HELP odoo_backup_last_run_ok 1 if the last finished backup succeeded.");
    let _ = writeln!(out, "# TYPE odoo_backup_last_run_ok gauge");
    let _ = writeln!(out, "# HELP odoo_backup_stale 1 if a protected database has no recent successful backup.");
    let _ = writeln!(out, "# TYPE odoo_backup_stale gauge");
    for h in &health {
        let labels = format!("instance=\"{}\",database=\"{}\"", label(&h.instance_name), label(&h.database));
        if let Some(status) = &h.last_status {
            let _ = writeln!(out, "odoo_backup_last_run_ok{{{labels}}} {}", u8::from(status == "success"));
        }
        if h.protected {
            let _ = writeln!(out, "odoo_backup_stale{{{labels}}} {}", u8::from(h.stale));
        }
    }
    let counts: Vec<(String, i64)> =
        db::query_as("SELECT status, COUNT(*) FROM runs WHERE kind = 'backup' GROUP BY status")
            .fetch_all(&state.db)
            .await?;
    let _ = writeln!(out, "# HELP odoo_backup_runs Backup runs recorded, by status.");
    let _ = writeln!(out, "# TYPE odoo_backup_runs gauge");
    for (status, count) in counts {
        let _ = writeln!(out, "odoo_backup_runs{{status=\"{}\"}} {count}", label(&status));
    }
    let storage: Vec<(String, i64, i64)> = db::query_as(
        "SELECT d.name, COUNT(a.id), CAST(COALESCE(SUM(a.size_bytes), 0) AS BIGINT) FROM destinations d \
         LEFT JOIN artifacts a ON a.destination_id = d.id AND a.status = 'stored' GROUP BY d.id, d.name",
    )
    .fetch_all(&state.db)
    .await?;
    let _ = writeln!(out, "# HELP odoo_backup_destination_bytes Bytes of backups kept per destination.");
    let _ = writeln!(out, "# TYPE odoo_backup_destination_bytes gauge");
    for (name, _, bytes) in &storage {
        let _ = writeln!(out, "odoo_backup_destination_bytes{{destination=\"{}\"}} {bytes}", label(name));
    }
    let _ = writeln!(out, "# HELP odoo_backup_destination_files Backups kept per destination.");
    let _ = writeln!(out, "# TYPE odoo_backup_destination_files gauge");
    for (name, files, _) in &storage {
        let _ = writeln!(out, "odoo_backup_destination_files{{destination=\"{}\"}} {files}", label(name));
    }
    let _ = writeln!(out, "# HELP odoo_backup_active_runs Runs queued or in progress.");
    let _ = writeln!(out, "# TYPE odoo_backup_active_runs gauge");
    let _ = writeln!(out, "odoo_backup_active_runs {}", state.engine.active_count());

    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response())
}
