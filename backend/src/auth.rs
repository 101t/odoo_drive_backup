//! Password hashing, cookie sessions, the `AuthUser` extractor and login throttling.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, header};
use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::crypto::{random_token, sha256_hex};
use crate::error::AppError;
use crate::state::AppState;
use crate::util::now;

pub const SESSION_COOKIE: &str = "odb_session";

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        current_user(&state.db, &parts.headers).await?.ok_or(AppError::Unauthorized)
    }
}

pub async fn current_user(db: &SqlitePool, headers: &HeaderMap) -> Result<Option<AuthUser>, AppError> {
    let Some(token) = cookie_value(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT u.id, u.username FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = ? AND s.expires_at > ?",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(now())
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, username)| AuthUser { id, username }))
}

pub async fn create_session(db: &SqlitePool, user_id: i64, hours: i64) -> sqlx::Result<String> {
    let token = random_token();
    let created = now();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(sha256_hex(token.as_bytes()))
        .bind(user_id)
        .bind(created)
        .bind(created + chrono::Duration::hours(hours.clamp(1, 24 * 90)))
        .execute(db)
        .await?;
    Ok(token)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(sha256_hex(token.as_bytes())).execute(db).await?;
    Ok(())
}

pub fn session_cookie(token: &str, hours: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    let max_age = hours.clamp(1, 24 * 90) * 3600;
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cookie is ASCII")
}

pub fn clear_cookie(secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}"))
        .expect("cookie is ASCII")
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let hash: PasswordHash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Burns the same CPU time as a real verification so unknown usernames are not detectable.
pub fn dummy_verify(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let hash = DUMMY.get_or_init(|| hash_password("not-a-real-password").unwrap_or_default());
    let _ = verify_password(password, hash);
}

pub fn validate_username(username: &str) -> Result<(), AppError> {
    let ok =
        (3..=64).contains(&username.len()) && username.chars().all(|c| c.is_ascii_alphanumeric() || "._@-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(AppError::bad_request("The username must be 3-64 characters (letters, digits, '.', '_', '@', '-')"))
    }
}

pub fn validate_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < 10 {
        return Err(AppError::bad_request("The password must be at least 10 characters long"));
    }
    if password.len() > 512 {
        return Err(AppError::bad_request("The password is too long"));
    }
    Ok(())
}

pub async fn purge_expired_sessions(db: &SqlitePool) -> sqlx::Result<u64> {
    let res = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(Utc::now()).execute(db).await?;
    Ok(res.rows_affected())
}

/// Per-username exponential lockout after repeated failed logins.
#[derive(Default)]
pub struct LoginLimiter {
    entries: Mutex<HashMap<String, (u32, Option<Instant>)>>,
}

const FREE_ATTEMPTS: u32 = 5;

impl LoginLimiter {
    pub fn check(&self, key: &str) -> Result<(), AppError> {
        let map = self.entries.lock().unwrap();
        if let Some((_, Some(until))) = map.get(key) {
            let now = Instant::now();
            if *until > now {
                let secs = (*until - now).as_secs() + 1;
                return Err(AppError::TooManyRequests(format!(
                    "Too many failed login attempts. Try again in {secs} seconds."
                )));
            }
        }
        Ok(())
    }

    pub fn failure(&self, key: &str) {
        let mut map = self.entries.lock().unwrap();
        let entry = map.entry(key.to_string()).or_insert((0, None));
        entry.0 += 1;
        if entry.0 >= FREE_ATTEMPTS {
            let exp = (entry.0 - FREE_ATTEMPTS).min(5);
            let lock = Duration::from_secs(30 * (1 << exp)).min(Duration::from_secs(900));
            entry.1 = Some(Instant::now() + lock);
        }
    }

    pub fn success(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn cookie_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("a=1; odb_session=abc; b=2"));
        assert_eq!(cookie_value(&headers, SESSION_COOKIE).as_deref(), Some("abc"));
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn limiter_locks_after_failures() {
        let limiter = LoginLimiter::default();
        for _ in 0..FREE_ATTEMPTS - 1 {
            limiter.failure("admin");
            assert!(limiter.check("admin").is_ok());
        }
        limiter.failure("admin");
        assert!(limiter.check("admin").is_err());
        assert!(limiter.check("other").is_ok());
        limiter.success("admin");
        assert!(limiter.check("admin").is_ok());
    }
}
