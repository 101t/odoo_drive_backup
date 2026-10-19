use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Column list shared by every query returning [`Run`]. A macro so it can be used in `concat!`.
#[macro_export]
macro_rules! run_select {
    () => {
        "SELECT r.id, r.kind, r.job_id, j.name AS job_name, r.instance_id, r.instance_name, \
         r.database, r.format, r.trigger, r.status, r.phase, r.error, r.file_name, r.size_bytes, \
         r.sha256, r.md5, r.odoo_version, r.source_artifact_id, r.restore_options, r.attempt, \
         r.created_at, r.started_at, r.finished_at \
         FROM runs r LEFT JOIN jobs j ON j.id = r.job_id"
    };
}

pub mod status {
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const SUCCESS: &str = "success";
    pub const PARTIAL: &str = "partial";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";

    pub const ARTIFACT_PENDING: &str = "pending";
    pub const ARTIFACT_UPLOADING: &str = "uploading";
    pub const ARTIFACT_STORED: &str = "stored";
    pub const ARTIFACT_FAILED: &str = "failed";
    /// Never attempted because the run failed or was cancelled before the upload phase.
    pub const ARTIFACT_SKIPPED: &str = "skipped";
    pub const ARTIFACT_DELETED: &str = "deleted";
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstanceRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub master_password_enc: Option<String>,
    pub verify_tls: bool,
    pub timeout_secs: i64,
    pub notes: String,
    pub odoo_version: Option<String>,
    pub databases_json: String,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_check_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InstanceRow {
    pub fn databases(&self) -> Vec<String> {
        serde_json::from_str(&self.databases_json).unwrap_or_default()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DestinationRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub secret_enc: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JobRow {
    pub id: i64,
    pub name: String,
    pub instance_id: i64,
    pub all_databases: bool,
    pub databases_json: String,
    pub format: String,
    pub cron: String,
    pub timezone: String,
    pub enabled: bool,
    pub keep_last: i64,
    pub keep_daily: i64,
    pub keep_weekly: i64,
    pub keep_monthly: i64,
    pub keep_yearly: i64,
    pub retries: i64,
    pub verify_archive: bool,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl JobRow {
    pub fn databases(&self) -> Vec<String> {
        serde_json::from_str(&self.databases_json).unwrap_or_default()
    }

    pub fn retention(&self) -> crate::engine::retention::RetentionPolicy {
        let n = |v: i64| v.clamp(0, 10_000) as u32;
        crate::engine::retention::RetentionPolicy {
            keep_last: n(self.keep_last),
            keep_daily: n(self.keep_daily),
            keep_weekly: n(self.keep_weekly),
            keep_monthly: n(self.keep_monthly),
            keep_yearly: n(self.keep_yearly),
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: i64,
    pub kind: String,
    pub job_id: Option<i64>,
    pub job_name: Option<String>,
    pub instance_id: Option<i64>,
    pub instance_name: String,
    pub database: String,
    pub format: String,
    pub trigger: String,
    pub status: String,
    pub phase: Option<String>,
    pub error: Option<String>,
    pub file_name: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub md5: Option<String>,
    pub odoo_version: Option<String>,
    pub source_artifact_id: Option<i64>,
    pub restore_options: Option<String>,
    pub attempt: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Run {
    pub fn is_active(&self) -> bool {
        self.status == status::QUEUED || self.status == status::RUNNING
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Artifact {
    pub id: i64,
    pub run_id: i64,
    pub destination_id: Option<i64>,
    pub destination_name: String,
    pub destination_kind: String,
    pub status: String,
    pub remote_id: Option<String>,
    pub remote_path: Option<String>,
    pub web_url: Option<String>,
    pub size_bytes: Option<i64>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub stored_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub delete_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RunLog {
    pub id: i64,
    pub run_id: i64,
    pub ts: DateTime<Utc>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NotificationChannelRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub secret_enc: Option<String>,
    pub on_success: bool,
    pub on_failure: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Options stored with a restore run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOptions {
    pub copy: bool,
    pub neutralize: bool,
    pub source_run_id: i64,
    pub source_instance: String,
    pub source_database: String,
}
