//! Serves the React single-page app embedded into the binary at compile time.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use serde_json::json;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
#[allow_missing = true]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path.starts_with("api/") || path == "api" {
        return (StatusCode::NOT_FOUND, axum::Json(json!({ "error": "Unknown API endpoint" }))).into_response();
    }

    if !path.is_empty()
        && let Some(file) = Assets::get(path)
    {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Vite fingerprints everything under assets/, so it can be cached forever.
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return Response::builder()
            .header(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap())
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(file.data.into_owned()))
            .unwrap();
    }

    // Client-side routes fall back to index.html.
    match Assets::get("index.html") {
        Some(index) => Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(index.data.into_owned()))
            .unwrap(),
        None => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<!doctype html><title>Odoo Backup</title><body style=\"font-family:sans-serif;padding:2rem\">\
             <h1>Odoo Backup API is running</h1><p>The web interface was not embedded in this build. \
             Build it with <code>make build</code> (or <code>npm run build</code> in <code>frontend/</code>) \
             and recompile the backend.</p></body>",
        )
            .into_response(),
    }
}
