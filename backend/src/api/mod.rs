//! REST API (`/api/*`), the SSE event stream and the Prometheus endpoint.

mod auth;
mod dashboard;
mod destinations;
mod events;
mod instances;
mod jobs;
mod notifications;
mod runs;
pub(crate) mod settings;

use axum::http::{HeaderValue, header};
use axum::routing::{delete, get, post};
use axum::{Json, Router, middleware};
use serde_json::{Value, json};
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::web;

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/auth/password", post(auth::change_password))
        .route("/api/dashboard", get(dashboard::overview))
        .route("/api/events", get(events::stream))
        .route("/api/instances", get(instances::list).post(instances::create))
        .route("/api/instances/test", post(instances::test_connection))
        .route("/api/instances/{id}", get(instances::get_one).put(instances::update).delete(instances::remove))
        .route("/api/instances/{id}/refresh", post(instances::refresh))
        .route("/api/destinations", get(destinations::list).post(destinations::create))
        .route(
            "/api/destinations/{id}",
            get(destinations::get_one).put(destinations::update).delete(destinations::remove),
        )
        .route("/api/destinations/{id}/test", post(destinations::test))
        .route("/api/destinations/{id}/oauth/start", post(destinations::oauth_start))
        .route("/api/oauth/google/callback", get(destinations::oauth_callback))
        .route("/api/jobs", get(jobs::list).post(jobs::create))
        .route("/api/jobs/{id}", get(jobs::get_one).put(jobs::update).delete(jobs::remove))
        .route("/api/jobs/{id}/run", post(jobs::run_now))
        .route("/api/cron/preview", post(jobs::cron_preview))
        .route("/api/backups", post(runs::backup_now))
        .route("/api/runs", get(runs::list))
        .route("/api/runs/{id}", get(runs::get_one).delete(runs::remove))
        .route("/api/runs/{id}/cancel", post(runs::cancel))
        .route("/api/artifacts/{id}", delete(runs::delete_artifact))
        .route("/api/artifacts/{id}/download", get(runs::download_artifact))
        .route("/api/artifacts/{id}/pin", axum::routing::put(runs::pin_artifact))
        .route("/api/artifacts/{id}/restore", post(runs::restore_artifact))
        .route("/api/notifications", get(notifications::list).post(notifications::create))
        .route("/api/notifications/{id}", axum::routing::put(notifications::update).delete(notifications::remove))
        .route("/api/notifications/{id}/test", post(notifications::test))
        .route("/api/settings", get(settings::get_settings).put(settings::update_settings))
        .route_layer(middleware::from_extractor_with_state::<AuthUser, AppState>(state.clone()));

    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/status", get(auth::status))
        .route("/api/auth/setup", post(auth::setup))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/metrics", get(dashboard::metrics));

    public
        .merge(protected)
        .fallback(web::static_handler)
        // Backup downloads are already compressed and can be huge: stream them untouched.
        .layer(
            CompressionLayer::new().compress_when(
                tower_http::compression::DefaultPredicate::new()
                    .and(NotForContentType::const_new("application/octet-stream")),
            ),
        )
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("same-origin")))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Validates an Odoo database name (same rule as Odoo's DBNAME_PATTERN).
pub(crate) fn valid_database_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    name.len() >= 2 && name.len() <= 63 && chars.all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

/// Deserializes `"1"`, `1`, `"true"` style query parameters into booleans.
pub(crate) fn truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "yes" | "on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_names() {
        assert!(valid_database_name("acme_prod"));
        assert!(valid_database_name("db-2026.restore"));
        assert!(!valid_database_name("_hidden"));
        assert!(!valid_database_name("a"));
        assert!(!valid_database_name("bad name"));
        assert!(!valid_database_name("drop;table"));
    }
}
