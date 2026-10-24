//! Exercises the resumable-upload client against a mock Google Drive that drops connections,
//! acknowledges partial chunks and expires sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use md5::{Digest, Md5};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{GdriveAuthKind, GdriveConfig, GdriveSecret, GdriveStorage};
use crate::storage::{StorageContext, UploadRequest};

#[derive(Default)]
struct Mock {
    base: String,
    sessions: HashMap<u32, Vec<u8>>,
    next_session: u32,
    puts: u32,
    /// PUT numbers that receive only half of the chunk, then answer 503.
    fail_puts: Vec<u32>,
    /// PUT numbers answered with 404 (session expired).
    expire_puts: Vec<u32>,
    saw_chunked_encoding: bool,
    misaligned_chunk: bool,
    files: HashMap<String, Vec<u8>>,
    folders_created: Vec<String>,
}

type Shared = Arc<Mutex<Mock>>;

fn range_header(len: usize) -> Vec<(&'static str, String)> {
    if len == 0 { vec![] } else { vec![("range", format!("bytes=0-{}", len - 1))] }
}

fn resume_incomplete(len: usize) -> Response {
    let mut response = StatusCode::PERMANENT_REDIRECT.into_response();
    for (name, value) in range_header(len) {
        response.headers_mut().insert(name, value.parse().unwrap());
    }
    response
}

async fn token() -> impl IntoResponse {
    axum::Json(json!({ "access_token": "test-token", "expires_in": 3600 }))
}

async fn list_files() -> impl IntoResponse {
    axum::Json(json!({ "files": [] }))
}

async fn create_folder(State(mock): State<Shared>, body: Bytes) -> impl IntoResponse {
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let mut m = mock.lock().unwrap();
    m.folders_created.push(value["name"].as_str().unwrap().to_string());
    axum::Json(json!({ "id": format!("folder-{}", m.folders_created.len()) }))
}

async fn start_session(State(mock): State<Shared>, headers: HeaderMap) -> Response {
    assert_eq!(headers["authorization"], "Bearer test-token");
    assert!(headers.contains_key("x-upload-content-length"));
    let mut m = mock.lock().unwrap();
    m.next_session += 1;
    let id = m.next_session;
    m.sessions.insert(id, Vec::new());
    let mut response = StatusCode::OK.into_response();
    response.headers_mut().insert("location", format!("{}/session/{id}", m.base).parse().unwrap());
    response
}

async fn put_session(State(mock): State<Shared>, Path(id): Path<u32>, headers: HeaderMap, body: Bytes) -> Response {
    let mut m = mock.lock().unwrap();
    if headers.contains_key("transfer-encoding") {
        m.saw_chunked_encoding = true;
    }
    let range = headers["content-range"].to_str().unwrap().to_string();
    let total: usize = range.rsplit('/').next().unwrap().parse().unwrap();

    if range.starts_with("bytes */") {
        let received = m.sessions.get(&id).map(Vec::len);
        return match received {
            None => StatusCode::NOT_FOUND.into_response(),
            Some(len) if len == total => finish(&mut m, id),
            Some(len) => resume_incomplete(len),
        };
    }

    m.puts += 1;
    let put_number = m.puts;
    if m.expire_puts.contains(&put_number) {
        m.sessions.remove(&id);
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(received) = m.sessions.get(&id).map(Vec::len) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let span = range.trim_start_matches("bytes ").split('/').next().unwrap();
    let (start, end) = span.split_once('-').unwrap();
    let (start, end): (usize, usize) = (start.parse().unwrap(), end.parse().unwrap());
    assert_eq!(start, received, "client must resume exactly where the server stopped");
    assert_eq!(end - start + 1, body.len(), "Content-Range must match the body");
    if end + 1 != total && !body.len().is_multiple_of(256 * 1024) {
        m.misaligned_chunk = true;
    }

    if m.fail_puts.contains(&put_number) {
        // Accept half of the chunk, then "lose the connection".
        let half = body.len() / 2;
        m.sessions.get_mut(&id).unwrap().extend_from_slice(&body[..half]);
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let data = m.sessions.get_mut(&id).unwrap();
    data.extend_from_slice(&body);
    let len = data.len();
    if len == total { finish(&mut m, id) } else { resume_incomplete(len) }
}

fn finish(m: &mut Mock, session: u32) -> Response {
    let data = m.sessions.get(&session).cloned().unwrap_or_default();
    let file_id = format!("file-{session}");
    m.files.insert(file_id.clone(), data);
    axum::Json(json!({ "id": file_id })).into_response()
}

async fn file_metadata(State(mock): State<Shared>, Path(id): Path<String>) -> Response {
    let m = mock.lock().unwrap();
    match m.files.get(&id) {
        Some(data) => axum::Json(json!({
            "id": id,
            "name": "backup.zip",
            "size": data.len().to_string(),
            "md5Checksum": hex::encode(Md5::digest(data)),
            "webViewLink": format!("https://drive.example/{id}"),
        }))
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_file(State(mock): State<Shared>, Path(id): Path<String>) -> StatusCode {
    mock.lock().unwrap().files.remove(&id);
    StatusCode::NO_CONTENT
}

async fn start_mock(mock: Shared) -> String {
    let app = Router::new()
        .route("/token", post(token))
        .route("/drive/v3/files", get(list_files).post(create_folder))
        .route("/drive/v3/files/{id}", get(file_metadata).delete(delete_file))
        .route("/upload/drive/v3/files", post(start_session))
        .route("/session/{id}", put(put_session))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    mock.lock().unwrap().base = base.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn storage(base: &str) -> GdriveStorage {
    let ctx = StorageContext::new(base.to_string(), format!("{base}/token"), 256 * 1024).unwrap();
    let config = GdriveConfig {
        auth: GdriveAuthKind::Oauth,
        folder_id: "root-folder".into(),
        subfolder_per_database: true,
        impersonate: None,
        client_id: Some("client".into()),
        service_account_email: None,
        account_email: None,
    };
    let secret = GdriveSecret {
        service_account_json: None,
        client_secret: Some("secret".into()),
        refresh_token: Some("refresh".into()),
    };
    GdriveStorage::new(format!("test-{base}"), &config, secret, ctx).unwrap()
}

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 31 % 251) as u8).collect()
}

async fn upload(storage: &GdriveStorage, data: &[u8]) -> anyhow::Result<crate::storage::StoredObject> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.zip");
    std::fs::write(&path, data).unwrap();
    let hashes = crate::storage::Hashes { md5: hex::encode(Md5::digest(data)), sha256: String::new() };
    let folder = vec!["Prod".to_string(), "acme".to_string()];
    let request = UploadRequest {
        file: &path,
        size: data.len() as u64,
        file_name: "backup.zip",
        folder: &folder,
        hashes: &hashes,
        mime: "application/zip",
    };
    let progress: crate::util::ProgressFn = Arc::new(|_| {});
    storage.upload(&request, progress, &CancellationToken::new()).await
}

#[tokio::test]
async fn upload_survives_dropped_chunks() {
    let mock: Shared = Arc::new(Mutex::new(Mock { fail_puts: vec![2, 4], ..Default::default() }));
    let base = start_mock(mock.clone()).await;
    let data = payload(5 * 256 * 1024 + 12_345);

    let stored = upload(&storage(&base), &data).await.expect("upload should succeed");

    let m = mock.lock().unwrap();
    assert_eq!(m.files.get(&stored.remote_id).unwrap(), &data, "Drive must hold the exact bytes");
    assert_eq!(m.folders_created, vec!["Prod", "acme"]);
    assert!(!m.saw_chunked_encoding, "chunks must be sent with Content-Length");
    assert!(!m.misaligned_chunk, "non-final chunks must be multiples of 256 KiB");
    assert_eq!(stored.remote_path, "Google Drive / Prod / acme / backup.zip");
    assert_eq!(stored.web_url.as_deref(), Some(format!("https://drive.example/{}", stored.remote_id).as_str()));
}

#[tokio::test]
async fn upload_restarts_expired_session() {
    let mock: Shared = Arc::new(Mutex::new(Mock { expire_puts: vec![2], ..Default::default() }));
    let base = start_mock(mock.clone()).await;
    let data = payload(3 * 256 * 1024);

    let stored = upload(&storage(&base), &data).await.expect("upload should succeed");

    let m = mock.lock().unwrap();
    assert_eq!(m.next_session, 2, "a new session must be opened after expiry");
    assert_eq!(m.files.get(&stored.remote_id).unwrap(), &data);
}

#[tokio::test]
async fn folders_are_cached_between_uploads() {
    let mock: Shared = Arc::new(Mutex::new(Mock::default()));
    let base = start_mock(mock.clone()).await;
    let storage = storage(&base);
    upload(&storage, &payload(1000)).await.unwrap();
    upload(&storage, &payload(2000)).await.unwrap();
    assert_eq!(mock.lock().unwrap().folders_created.len(), 2);
}

#[tokio::test]
async fn small_file_single_request() {
    let mock: Shared = Arc::new(Mutex::new(Mock::default()));
    let base = start_mock(mock.clone()).await;
    let data = payload(10);
    let stored = upload(&storage(&base), &data).await.unwrap();
    assert_eq!(mock.lock().unwrap().files.get(&stored.remote_id).unwrap(), &data);
    assert_eq!(mock.lock().unwrap().puts, 1);
}
