use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::NotificationChannelRow;
use crate::error::{ApiResult, AppError};
use crate::notify::{ChannelSecret, EmailConfig, TelegramConfig, WebhookConfig};
use crate::state::AppState;
use crate::util::now;

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ChannelInput {
    name: String,
    kind: String,
    config: Value,
    #[serde(default)]
    secret: Option<ChannelSecret>,
    #[serde(default)]
    on_success: bool,
    #[serde(default = "default_true")]
    on_failure: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ChannelView {
    id: i64,
    name: String,
    kind: String,
    config: Value,
    has_secret: bool,
    on_success: bool,
    on_failure: bool,
    enabled: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<NotificationChannelRow> for ChannelView {
    fn from(row: NotificationChannelRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            kind: row.kind,
            config: serde_json::from_str(&row.config_json).unwrap_or(Value::Null),
            has_secret: row.secret_enc.is_some(),
            on_success: row.on_success,
            on_failure: row.on_failure,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

async fn load(state: &AppState, id: i64) -> ApiResult<NotificationChannelRow> {
    sqlx::query_as("SELECT * FROM notification_channels WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Notification channel"))
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<ChannelView>>> {
    let rows: Vec<NotificationChannelRow> =
        sqlx::query_as("SELECT * FROM notification_channels ORDER BY name COLLATE NOCASE").fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

fn keep(new: Option<String>, old: Option<String>) -> Option<String> {
    new.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).or(old)
}

/// Validates the config for the channel type and merges secrets with the stored ones.
fn prepare(
    state: &AppState,
    input: &ChannelInput,
    existing: Option<&NotificationChannelRow>,
) -> ApiResult<(String, Option<String>)> {
    if input.name.trim().is_empty() || input.name.len() > 100 {
        return Err(AppError::bad_request("The name is required (max 100 characters)"));
    }
    let old: ChannelSecret = existing
        .and_then(|e| e.secret_enc.as_deref())
        .map(|enc| state.crypto.decrypt(enc))
        .transpose()?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let new = input.secret.clone().unwrap_or_default();
    let bad = |e: serde_json::Error| AppError::bad_request(format!("Invalid settings: {e}"));

    let (config, secret) = match input.kind.as_str() {
        "webhook" => {
            let config: WebhookConfig = serde_json::from_value(input.config.clone()).map_err(bad)?;
            let url = keep(new.url, old.url).ok_or_else(|| AppError::bad_request("The webhook URL is required"))?;
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(AppError::bad_request("The webhook URL must start with https://"));
            }
            (serde_json::to_value(config), ChannelSecret { url: Some(url), ..Default::default() })
        }
        "telegram" => {
            let config: TelegramConfig = serde_json::from_value(input.config.clone()).map_err(bad)?;
            if config.chat_id.trim().is_empty() {
                return Err(AppError::bad_request("The chat ID is required"));
            }
            let token =
                keep(new.bot_token, old.bot_token).ok_or_else(|| AppError::bad_request("The bot token is required"))?;
            (serde_json::to_value(config), ChannelSecret { bot_token: Some(token), ..Default::default() })
        }
        "email" => {
            let config: EmailConfig = serde_json::from_value(input.config.clone()).map_err(bad)?;
            if config.host.trim().is_empty() || config.from.trim().is_empty() || config.to.is_empty() {
                return Err(AppError::bad_request("SMTP host, sender and at least one recipient are required"));
            }
            (
                serde_json::to_value(config),
                ChannelSecret { password: keep(new.password, old.password), ..Default::default() },
            )
        }
        other => return Err(AppError::bad_request(format!("Unknown channel type '{other}'"))),
    };
    let config = config.map_err(|e| AppError::Internal(e.into()))?;
    let secret_json = serde_json::to_string(&secret).map_err(|e| AppError::Internal(e.into()))?;
    Ok((config.to_string(), Some(state.crypto.encrypt(&secret_json))))
}

pub async fn create(State(state): State<AppState>, Json(input): Json<ChannelInput>) -> ApiResult<Json<ChannelView>> {
    let (config, secret) = prepare(&state, &input, None)?;
    let ts = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO notification_channels (name, kind, config_json, secret_enc, on_success, on_failure, enabled, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&input.kind)
    .bind(config)
    .bind(secret)
    .bind(input.on_success)
    .bind(input.on_failure)
    .bind(input.enabled)
    .bind(ts)
    .bind(ts)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(load(&state, id).await?.into()))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ChannelInput>,
) -> ApiResult<Json<ChannelView>> {
    let existing = load(&state, id).await?;
    if existing.kind != input.kind {
        return Err(AppError::bad_request("The channel type cannot be changed"));
    }
    let (config, secret) = prepare(&state, &input, Some(&existing))?;
    sqlx::query(
        "UPDATE notification_channels SET name = ?, config_json = ?, secret_enc = ?, on_success = ?, on_failure = ?, \
         enabled = ?, updated_at = ? WHERE id = ?",
    )
    .bind(input.name.trim())
    .bind(config)
    .bind(secret)
    .bind(input.on_success)
    .bind(input.on_failure)
    .bind(input.enabled)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Json(load(&state, id).await?.into()))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let deleted = sqlx::query("DELETE FROM notification_channels WHERE id = ?").bind(id).execute(&state.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::not_found("Notification channel"));
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn test(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = load(&state, id).await?;
    let message = state.engine.notifier.test_message();
    match state.engine.notifier.send(&row, &message).await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(err) => Ok(Json(json!({ "ok": false, "error": format!("{err:#}") }))),
    }
}
