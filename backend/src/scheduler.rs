//! Cron scheduler. Each job stores its next due time; a run missed while the service was
//! down is executed once at startup, then the regular schedule resumes.

use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use tokio::task::JoinHandle;

use crate::db::{self, JobRow};
use crate::state::AppState;

pub fn parse_cron(expression: &str) -> anyhow::Result<Cron> {
    let expression = expression.trim();
    let fields = expression.split_whitespace().count();
    if fields != 5 {
        return Err(anyhow!(
            "a cron expression needs 5 fields (minute hour day-of-month month day-of-week), got {fields}"
        ));
    }
    Cron::from_str(expression).map_err(|e| anyhow!("invalid cron expression: {e}"))
}

pub fn parse_timezone(name: &str) -> anyhow::Result<Tz> {
    name.trim().parse::<Tz>().map_err(|_| anyhow!("unknown time zone \"{name}\""))
}

pub fn next_run(expression: &str, timezone: &str, after: DateTime<Utc>) -> anyhow::Result<DateTime<Utc>> {
    upcoming(expression, timezone, after, 1)?.into_iter().next().ok_or_else(|| anyhow!("this schedule never fires"))
}

pub fn upcoming(
    expression: &str,
    timezone: &str,
    after: DateTime<Utc>,
    count: usize,
) -> anyhow::Result<Vec<DateTime<Utc>>> {
    let cron = parse_cron(expression)?;
    let tz = parse_timezone(timezone)?;
    let mut cursor = after.with_timezone(&tz);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        match cron.find_next_occurrence(&cursor, false) {
            Ok(next) => {
                out.push(next.with_timezone(&Utc));
                cursor = next;
            }
            Err(_) => break,
        }
    }
    Ok(out)
}

pub fn describe(expression: &str) -> String {
    parse_cron(expression).map(|c| c.describe()).unwrap_or_default()
}

pub fn spawn(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(err) = fill_missing_next_runs(&state).await {
            tracing::error!(error = %format!("{err:#}"), "scheduler initialisation failed");
        }
        let mut last_housekeeping = Instant::now() - Duration::from_secs(3600);
        loop {
            if let Err(err) = tick(&state).await {
                tracing::error!(error = %format!("{err:#}"), "scheduler tick failed");
            }
            if last_housekeeping.elapsed() >= Duration::from_secs(3600) {
                last_housekeeping = Instant::now();
                housekeeping(&state).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(20)) => {}
                _ = state.scheduler_wake.notified() => {}
                _ = state.shutdown.cancelled() => break,
            }
        }
        tracing::info!("scheduler stopped");
    })
}

async fn fill_missing_next_runs(state: &AppState) -> anyhow::Result<()> {
    let jobs: Vec<JobRow> =
        sqlx::query_as("SELECT * FROM jobs WHERE enabled = 1 AND next_run_at IS NULL").fetch_all(&state.db).await?;
    for job in jobs {
        let next = next_run(&job.cron, &job.timezone, Utc::now()).ok();
        sqlx::query("UPDATE jobs SET next_run_at = ? WHERE id = ?").bind(next).bind(job.id).execute(&state.db).await?;
    }
    Ok(())
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let now = Utc::now();
    let due: Vec<JobRow> = sqlx::query_as(
        "SELECT * FROM jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? ORDER BY next_run_at",
    )
    .bind(now)
    .fetch_all(&state.db)
    .await
    .context("loading due jobs")?;

    for job in due {
        let next = match next_run(&job.cron, &job.timezone, now) {
            Ok(next) => Some(next),
            Err(err) => {
                tracing::error!(job = %job.name, error = %err, "invalid schedule, disabling job");
                None
            }
        };
        sqlx::query("UPDATE jobs SET next_run_at = ?, last_run_at = ? WHERE id = ?")
            .bind(next)
            .bind(now)
            .bind(job.id)
            .execute(&state.db)
            .await?;
        if let Some(scheduled) = job.next_run_at
            && now - scheduled > chrono::Duration::minutes(5)
        {
            tracing::warn!(job = %job.name, %scheduled, "running a missed scheduled backup");
        }
        let engine = state.engine.clone();
        tokio::spawn(async move {
            if let Err(err) = engine.trigger_job(job.id, "schedule").await {
                tracing::error!(job = %job.name, error = %format!("{err:#}"), "cannot start scheduled job");
            }
        });
    }
    Ok(())
}

async fn housekeeping(state: &AppState) {
    if let Err(err) = crate::auth::purge_expired_sessions(&state.db).await {
        tracing::warn!(error = %err, "session cleanup failed");
    }
    let days: i64 = db::get_setting(&state.db, "log_retention_days")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(90);
    if days > 0 {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        if let Err(err) = sqlx::query("DELETE FROM run_logs WHERE ts < ?").bind(cutoff).execute(&state.db).await {
            tracing::warn!(error = %err, "log cleanup failed");
        }
    }
    state.oauth_states.lock().unwrap().retain(|_, pending| pending.created.elapsed() < Duration::from_secs(900));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn daily_schedule_in_timezone() {
        let after = Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap();
        // 02:00 in Beirut (UTC+3 in October) is 23:00 UTC the previous day.
        let next = next_run("0 2 * * *", "Asia/Beirut", after).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 10, 19, 23, 0, 0).unwrap());
    }

    #[test]
    fn upcoming_runs_are_increasing() {
        let after = Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap();
        let runs = upcoming("*/30 * * * *", "UTC", after, 4).unwrap();
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[0], Utc.with_ymd_and_hms(2026, 10, 19, 12, 30, 0).unwrap());
        assert!(runs.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(parse_cron("0 2 * *").is_err());
        assert!(parse_cron("0 25 * * *").is_err());
        assert!(parse_cron("* * * * * *").is_err());
        assert!(parse_timezone("Mars/Base").is_err());
        assert!(!describe("0 2 * * 0").is_empty());
    }
}
