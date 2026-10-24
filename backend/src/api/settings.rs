use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{Db, get_setting, set_setting};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;
use crate::util::available_space;

pub const DEFAULT_STALE_HOURS: i64 = 26;
pub const DEFAULT_LOG_RETENTION_DAYS: i64 = 90;
pub const DEFAULT_SIZE_DROP_WARN_PERCENT: i64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    /// A protected database without a successful backup for this long is flagged as stale.
    pub stale_after_hours: i64,
    /// Run log lines older than this are pruned (0 = keep forever).
    pub log_retention_days: i64,
    /// A backup this many percent smaller than the previous one gets a warning (0 = off).
    #[serde(default = "default_size_drop")]
    pub size_drop_warn_percent: i64,
}

fn default_size_drop() -> i64 {
    DEFAULT_SIZE_DROP_WARN_PERCENT
}

pub async fn load(db: &Db) -> sqlx::Result<Settings> {
    let number = |v: Option<String>, default: i64| v.and_then(|s| s.parse().ok()).unwrap_or(default);
    Ok(Settings {
        stale_after_hours: number(get_setting(db, "stale_after_hours").await?, DEFAULT_STALE_HOURS),
        log_retention_days: number(get_setting(db, "log_retention_days").await?, DEFAULT_LOG_RETENTION_DAYS),
        size_drop_warn_percent: number(
            get_setting(db, "size_drop_warn_percent").await?,
            DEFAULT_SIZE_DROP_WARN_PERCENT,
        ),
    })
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let settings = load(&state.db).await?;
    let temp_dir = state.config.temp_dir();
    Ok(Json(json!({
        "settings": settings,
        "system": {
            "version": env!("CARGO_PKG_VERSION"),
            "data_dir": state.config.data_dir.display().to_string(),
            "database": state.db.describe().await,
            "temp_dir": temp_dir.display().to_string(),
            "temp_free_bytes": available_space(&temp_dir),
            "max_concurrent_jobs": state.config.max_concurrent_jobs,
            "public_url": state.config.public_url(),
            "metrics_enabled": state.config.metrics_token.is_some(),
            "started_at": state.started_at,
            "active_runs": state.engine.active_count(),
        }
    })))
}

pub async fn update_settings(State(state): State<AppState>, Json(input): Json<Settings>) -> ApiResult<Json<Value>> {
    if !(1..=24 * 365).contains(&input.stale_after_hours) {
        return Err(AppError::bad_request("stale_after_hours must be between 1 and 8760"));
    }
    if !(0..=3650).contains(&input.log_retention_days) {
        return Err(AppError::bad_request("log_retention_days must be between 0 and 3650"));
    }
    if !(0..=99).contains(&input.size_drop_warn_percent) {
        return Err(AppError::bad_request("size_drop_warn_percent must be between 0 and 99"));
    }
    set_setting(&state.db, "stale_after_hours", &input.stale_after_hours.to_string()).await?;
    set_setting(&state.db, "log_retention_days", &input.log_retention_days.to_string()).await?;
    set_setting(&state.db, "size_drop_warn_percent", &input.size_drop_warn_percent.to_string()).await?;
    get_settings(State(state)).await
}
