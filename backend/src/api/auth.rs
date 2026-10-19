use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{
    AuthUser, SESSION_COOKIE, clear_cookie, cookie_value, create_session, current_user, delete_session, dummy_verify,
    hash_password, session_cookie, validate_password, validate_username, verify_password,
};
use crate::db::UserRow;
use crate::error::{ApiResult, AppError};
use crate::state::AppState;
use crate::util::now;

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct PasswordChange {
    current_password: String,
    new_password: String,
}

pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    let user = current_user(&state.db, &headers).await?;
    Ok(Json(json!({
        "setup_required": users == 0,
        "user": user,
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

pub async fn setup(State(state): State<AppState>, Json(input): Json<Credentials>) -> ApiResult<Response> {
    let username = input.username.trim().to_string();
    validate_username(&username)?;
    validate_password(&input.password)?;
    let password = input.password.clone();
    let hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AppError::Internal(e.into()))??;
    // Atomic: only succeeds while no user exists.
    let inserted = sqlx::query(
        "INSERT INTO users (username, password_hash, created_at) SELECT ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(&username)
    .bind(hash)
    .bind(now())
    .execute(&state.db)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(AppError::Conflict("The initial setup has already been completed".into()));
    }
    let id: i64 =
        sqlx::query_scalar("SELECT id FROM users WHERE username = ?").bind(&username).fetch_one(&state.db).await?;
    tracing::info!(username = %username, "initial admin account created");
    signed_in(&state, AuthUser { id, username }).await
}

pub async fn login(State(state): State<AppState>, Json(input): Json<Credentials>) -> ApiResult<Response> {
    let key = input.username.trim().to_lowercase();
    state.limiter.check(&key)?;

    let user: Option<UserRow> = sqlx::query_as("SELECT * FROM users WHERE username = ?")
        .bind(input.username.trim())
        .fetch_optional(&state.db)
        .await?;
    let password = input.password.clone();
    let hash = user.as_ref().map(|u| u.password_hash.clone());
    let valid = tokio::task::spawn_blocking(move || match hash {
        Some(hash) => verify_password(&password, &hash),
        None => {
            dummy_verify(&password);
            false
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    match user {
        Some(user) if valid => {
            state.limiter.success(&key);
            signed_in(&state, AuthUser { id: user.id, username: user.username }).await
        }
        _ => {
            state.limiter.failure(&key);
            tracing::warn!(username = %key, "failed login attempt");
            Err(AppError::bad_request("Invalid username or password"))
        }
    }
}

async fn signed_in(state: &AppState, user: AuthUser) -> ApiResult<Response> {
    let token = create_session(&state.db, user.id, state.config.session_hours).await?;
    let cookie = session_cookie(&token, state.config.session_hours, state.config.cookie_secure);
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({ "user": user }))).into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        delete_session(&state.db, &token).await?;
    }
    Ok(([(header::SET_COOKIE, clear_cookie(state.config.cookie_secure))], Json(json!({ "ok": true }))).into_response())
}

pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(input): Json<PasswordChange>,
) -> ApiResult<Json<serde_json::Value>> {
    validate_password(&input.new_password)?;
    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?").bind(user.id).fetch_one(&state.db).await?;
    let (current, new) = (input.current_password.clone(), input.new_password.clone());
    let new_hash = tokio::task::spawn_blocking(move || {
        if verify_password(&current, &stored) { hash_password(&new).map(Some) } else { Ok(None) }
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??
    .ok_or_else(|| AppError::bad_request("The current password is incorrect"))?;

    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    // Sign out every other session of this user.
    let keep =
        cookie_value(&headers, SESSION_COOKIE).map(|t| crate::crypto::sha256_hex(t.as_bytes())).unwrap_or_default();
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
        .bind(user.id)
        .bind(keep)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
