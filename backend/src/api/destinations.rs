use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::crypto::random_token;
use crate::db::{DestinationRow, get_destination};
use crate::error::{ApiResult, AppError};
use crate::state::{AppState, PendingOAuth};
use crate::storage::gdrive::{self, ServiceAccountKey, extract_folder_id};
use crate::storage::{GdriveAuthKind, GdriveConfig, GdriveSecret, LocalConfig, Storage, TestReport};
use crate::util::now;

#[derive(Debug, Deserialize)]
pub struct DestinationInput {
    name: String,
    kind: String,
    config: Value,
    /// Only the provided, non-empty fields replace stored secrets.
    #[serde(default)]
    secret: Option<GdriveSecret>,
}

#[derive(Debug, Serialize)]
pub struct DestinationView {
    id: i64,
    name: String,
    kind: String,
    config: Value,
    has_service_account_key: bool,
    has_client_secret: bool,
    connected: bool,
    stored_files: i64,
    stored_bytes: i64,
    job_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DestinationListRow {
    #[sqlx(flatten)]
    destination: DestinationRow,
    stored_files: i64,
    stored_bytes: i64,
    job_count: i64,
}

macro_rules! destination_select {
    () => {
        "SELECT d.*, \
           (SELECT COUNT(*) FROM artifacts a WHERE a.destination_id = d.id AND a.status = 'stored') AS stored_files, \
           (SELECT COALESCE(SUM(a.size_bytes), 0) FROM artifacts a WHERE a.destination_id = d.id \
              AND a.status = 'stored') AS stored_bytes, \
           (SELECT COUNT(*) FROM job_destinations jd WHERE jd.destination_id = d.id) AS job_count \
         FROM destinations d"
    };
}

fn view(state: &AppState, row: DestinationListRow) -> DestinationView {
    let d = row.destination;
    let secret: GdriveSecret = d
        .secret_enc
        .as_deref()
        .and_then(|enc| state.crypto.decrypt(enc).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    DestinationView {
        id: d.id,
        name: d.name,
        kind: d.kind,
        config: serde_json::from_str(&d.config_json).unwrap_or(Value::Null),
        has_service_account_key: secret.service_account_json.is_some(),
        has_client_secret: secret.client_secret.is_some(),
        connected: secret.refresh_token.is_some(),
        stored_files: row.stored_files,
        stored_bytes: row.stored_bytes,
        job_count: row.job_count,
        created_at: d.created_at,
        updated_at: d.updated_at,
    }
}

async fn load_view(state: &AppState, id: i64) -> ApiResult<DestinationView> {
    let row: DestinationListRow = sqlx::query_as(concat!(destination_select!(), " WHERE d.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Destination"))?;
    Ok(view(state, row))
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<DestinationView>>> {
    let rows: Vec<DestinationListRow> =
        sqlx::query_as(concat!(destination_select!(), " ORDER BY d.name COLLATE NOCASE")).fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(|r| view(&state, r)).collect()))
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<DestinationView>> {
    Ok(Json(load_view(&state, id).await?))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Validates the input and merges it with the stored row (if any) into `(config_json, secret_json)`.
fn prepare(
    input: &DestinationInput,
    existing: Option<&DestinationRow>,
    state: &AppState,
) -> ApiResult<(String, Option<String>)> {
    if input.name.trim().is_empty() || input.name.len() > 100 {
        return Err(AppError::bad_request("The name is required (max 100 characters)"));
    }
    if let Some(existing) = existing
        && existing.kind != input.kind
    {
        return Err(AppError::bad_request("The destination type cannot be changed"));
    }
    match input.kind.as_str() {
        "gdrive" => {
            let mut config: GdriveConfig = serde_json::from_value(input.config.clone())
                .map_err(|e| AppError::bad_request(format!("Invalid Google Drive settings: {e}")))?;
            config.folder_id = extract_folder_id(&config.folder_id);
            config.impersonate = non_empty(config.impersonate.take());
            config.client_id = non_empty(config.client_id.take());

            let previous_config: Option<GdriveConfig> =
                existing.and_then(|e| serde_json::from_str(&e.config_json).ok());
            let mut secret: GdriveSecret = existing
                .and_then(|e| e.secret_enc.as_deref())
                .map(|enc| state.crypto.decrypt(enc))
                .transpose()?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let incoming = input.secret.clone().unwrap_or_default();

            match config.auth {
                GdriveAuthKind::ServiceAccount => {
                    if let Some(json) = non_empty(incoming.service_account_json) {
                        let key =
                            ServiceAccountKey::parse(&json).map_err(|e| AppError::bad_request(format!("{e:#}")))?;
                        config.service_account_email = Some(key.client_email);
                        secret.service_account_json = Some(json);
                    } else {
                        config.service_account_email =
                            previous_config.as_ref().and_then(|p| p.service_account_email.clone());
                    }
                    if secret.service_account_json.is_none() {
                        return Err(AppError::bad_request("Upload the service account JSON key"));
                    }
                    if config.folder_id.is_empty() && config.impersonate.is_none() {
                        return Err(AppError::bad_request(
                            "A folder ID is required: service accounts must write into a Shared Drive folder",
                        ));
                    }
                    secret.client_secret = None;
                    secret.refresh_token = None;
                    config.account_email = None;
                }
                GdriveAuthKind::Oauth => {
                    if config.client_id.is_none() {
                        return Err(AppError::bad_request("The OAuth client ID is required"));
                    }
                    if let Some(client_secret) = non_empty(incoming.client_secret) {
                        secret.client_secret = Some(client_secret);
                    }
                    if secret.client_secret.is_none() {
                        return Err(AppError::bad_request("The OAuth client secret is required"));
                    }
                    let same_client = previous_config
                        .as_ref()
                        .is_some_and(|p| p.client_id == config.client_id && p.auth == GdriveAuthKind::Oauth);
                    if let Some(token) = non_empty(incoming.refresh_token) {
                        secret.refresh_token = Some(token);
                    } else if !same_client {
                        // A refresh token is bound to its OAuth client.
                        secret.refresh_token = None;
                    }
                    config.account_email = if same_client && secret.refresh_token.is_some() {
                        previous_config.and_then(|p| p.account_email)
                    } else {
                        None
                    };
                    secret.service_account_json = None;
                    config.service_account_email = None;
                }
            }
            let config_json = serde_json::to_string(&config).map_err(|e| AppError::Internal(e.into()))?;
            let secret_json = serde_json::to_string(&secret).map_err(|e| AppError::Internal(e.into()))?;
            Ok((config_json, Some(state.crypto.encrypt(&secret_json))))
        }
        "local" => {
            let config: LocalConfig = serde_json::from_value(input.config.clone())
                .map_err(|e| AppError::bad_request(format!("Invalid local destination settings: {e}")))?;
            let path = config.path.trim();
            if !std::path::Path::new(path).is_absolute() {
                return Err(AppError::bad_request("The directory must be an absolute path, e.g. /backups"));
            }
            let config = LocalConfig { path: path.to_string(), ..config };
            Ok((serde_json::to_string(&config).map_err(|e| AppError::Internal(e.into()))?, None))
        }
        other => Err(AppError::bad_request(format!("Unknown destination type '{other}'"))),
    }
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<DestinationInput>,
) -> ApiResult<Json<DestinationView>> {
    let (config_json, secret_enc) = prepare(&input, None, &state)?;
    let ts = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO destinations (name, kind, config_json, secret_enc, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&input.kind)
    .bind(config_json)
    .bind(secret_enc)
    .bind(ts)
    .bind(ts)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(load_view(&state, id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<DestinationInput>,
) -> ApiResult<Json<DestinationView>> {
    let existing = get_destination(&state.db, id).await?;
    let (config_json, secret_enc) = prepare(&input, Some(&existing), &state)?;
    sqlx::query("UPDATE destinations SET name = ?, config_json = ?, secret_enc = ?, updated_at = ? WHERE id = ?")
        .bind(input.name.trim())
        .bind(config_json)
        .bind(secret_enc)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE artifacts SET destination_name = ? WHERE destination_id = ?")
        .bind(input.name.trim())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(load_view(&state, id).await?))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let jobs: Vec<String> = sqlx::query_scalar(
        "SELECT j.name FROM jobs j JOIN job_destinations jd ON jd.job_id = j.id WHERE jd.destination_id = ?",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    if !jobs.is_empty() {
        return Err(AppError::Conflict(format!(
            "This destination is used by: {}. Remove it from those jobs first.",
            jobs.join(", ")
        )));
    }
    let deleted = sqlx::query("DELETE FROM destinations WHERE id = ?").bind(id).execute(&state.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::not_found("Destination"));
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn test(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<TestReport>> {
    let row = get_destination(&state.db, id).await?;
    let report = match Storage::open(&row, &state.crypto, &state.storage) {
        Ok(storage) => storage.test().await,
        Err(err) => {
            let mut report = TestReport::default();
            report.fail("Configuration", format!("{err:#}"));
            report.finish()
        }
    };
    Ok(Json(report))
}

/// Base URL used to build the OAuth redirect URI.
fn external_base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = state.config.public_url() {
        return url;
    }
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let proto = get("x-forwarded-proto").unwrap_or_else(|| "http".into());
    let host =
        get("x-forwarded-host").or_else(|| get(header::HOST.as_str())).unwrap_or_else(|| state.config.bind.to_string());
    format!("{}://{}", proto.split(',').next().unwrap_or("http").trim(), host.split(',').next().unwrap_or("").trim())
}

pub async fn oauth_start(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let row = get_destination(&state.db, id).await?;
    let config: GdriveConfig = serde_json::from_str(&row.config_json)
        .map_err(|_| AppError::bad_request("Only Google Drive destinations can be connected"))?;
    if row.kind != "gdrive" || config.auth != GdriveAuthKind::Oauth {
        return Err(AppError::bad_request("This destination does not use OAuth"));
    }
    let client_id = config.client_id.ok_or_else(|| AppError::bad_request("The OAuth client ID is missing"))?;
    let redirect_uri = format!("{}/api/oauth/google/callback", external_base_url(&state, &headers));
    let token = random_token();
    let url = gdrive::authorize_url(&state.config.google_auth_url, &client_id, &redirect_uri, &token)?;
    state.oauth_states.lock().unwrap().insert(
        token,
        PendingOAuth { destination_id: id, redirect_uri: redirect_uri.clone(), created: Instant::now() },
    );
    Ok(Json(json!({ "url": url, "redirect_uri": redirect_uri })))
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub async fn oauth_callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> Response {
    match finish_oauth(&state, query).await {
        Ok(id) => Redirect::to(&format!("/destinations?connected={id}")).into_response(),
        Err(err) => {
            let message = url::form_urlencoded::byte_serialize(err.to_string().as_bytes()).collect::<String>();
            Redirect::to(&format!("/destinations?oauth_error={message}")).into_response()
        }
    }
}

async fn finish_oauth(state: &AppState, query: CallbackQuery) -> ApiResult<i64> {
    if let Some(error) = query.error {
        return Err(AppError::bad_request(format!("Google returned an error: {error}")));
    }
    let token = query.state.ok_or_else(|| AppError::bad_request("Missing state"))?;
    let code = query.code.ok_or_else(|| AppError::bad_request("Missing authorization code"))?;
    let pending = state
        .oauth_states
        .lock()
        .unwrap()
        .remove(&token)
        .ok_or_else(|| AppError::bad_request("This authorization link has expired; start again"))?;
    if pending.created.elapsed() > Duration::from_secs(900) {
        return Err(AppError::bad_request("This authorization link has expired; start again"));
    }

    let row = get_destination(&state.db, pending.destination_id).await?;
    let mut config: GdriveConfig = serde_json::from_str(&row.config_json).map_err(|e| AppError::Internal(e.into()))?;
    let mut secret: GdriveSecret = row
        .secret_enc
        .as_deref()
        .map(|enc| state.crypto.decrypt(enc))
        .transpose()?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let client_id = config.client_id.clone().ok_or_else(|| AppError::bad_request("OAuth client ID missing"))?;
    let client_secret =
        secret.client_secret.clone().ok_or_else(|| AppError::bad_request("OAuth client secret missing"))?;

    let tokens = gdrive::exchange_code(&state.storage, &client_id, &client_secret, &code, &pending.redirect_uri)
        .await
        .map_err(|e| AppError::bad_request(format!("{e:#}")))?;
    config.account_email = gdrive::account_email(&state.storage, &tokens.access_token).await;
    secret.refresh_token = Some(tokens.refresh_token);

    sqlx::query("UPDATE destinations SET config_json = ?, secret_enc = ?, updated_at = ? WHERE id = ?")
        .bind(serde_json::to_string(&config).map_err(|e| AppError::Internal(e.into()))?)
        .bind(state.crypto.encrypt(&serde_json::to_string(&secret).map_err(|e| AppError::Internal(e.into()))?))
        .bind(now())
        .bind(row.id)
        .execute(&state.db)
        .await?;
    tracing::info!(destination = %row.name, account = ?config.account_email, "Google account connected");
    Ok(row.id)
}
