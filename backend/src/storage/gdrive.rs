//! Google Drive destination: service-account (JWT) or OAuth refresh-token authentication,
//! resumable chunked uploads with automatic resume, and end-to-end MD5 verification.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, HeaderMap, LOCATION, RANGE};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::sync::CancellationToken;

use super::{ByteStream, StorageContext, StoredObject, TestReport, UploadRequest};
use crate::util::{Cancelled, ProgressFn, backoff, error_chain, format_bytes};

pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const DEFAULT_FOLDER_NAME: &str = "Odoo Backups";
const MAX_CHUNK_FAILURES: u32 = 12;
const MAX_API_ATTEMPTS: u32 = 6;
const MAX_SESSION_RESTARTS: u32 = 3;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GdriveAuthKind {
    ServiceAccount,
    Oauth,
}

/// Non-secret part of a Google Drive destination (stored in clear in `config_json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdriveConfig {
    pub auth: GdriveAuthKind,
    /// Target folder ID (a folder URL is accepted too). Empty with OAuth = "My Drive/Odoo Backups".
    #[serde(default)]
    pub folder_id: String,
    /// Store files under `<instance>/<database>/` sub-folders.
    #[serde(default = "default_true")]
    pub subfolder_per_database: bool,
    /// Service accounts only: user to impersonate through domain-wide delegation.
    #[serde(default)]
    pub impersonate: Option<String>,
    /// OAuth only: client ID of the Google Cloud OAuth client.
    #[serde(default)]
    pub client_id: Option<String>,
    /// Informational: e-mail of the service account (taken from the key).
    #[serde(default)]
    pub service_account_email: Option<String>,
    /// Informational: Google account connected through OAuth.
    #[serde(default)]
    pub account_email: Option<String>,
}

/// Secret part of a Google Drive destination (stored encrypted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GdriveSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_account_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccountKey {
    #[serde(rename = "type", default)]
    pub key_type: Option<String>,
    pub client_email: String,
    pub private_key: String,
    #[serde(default)]
    pub private_key_id: Option<String>,
}

impl ServiceAccountKey {
    pub fn parse(json: &str) -> anyhow::Result<Self> {
        let key: ServiceAccountKey = serde_json::from_str(json)
            .context("the service account key is not valid JSON (expected the file downloaded from Google Cloud)")?;
        if key.key_type.as_deref().is_some_and(|t| t != "service_account") {
            bail!("this JSON is not a service account key (type = {:?})", key.key_type.unwrap_or_default());
        }
        if !key.private_key.contains("PRIVATE KEY") {
            bail!("the service account key has no private key");
        }
        Ok(key)
    }
}

/// Process-wide caches for access tokens and folder IDs.
#[derive(Default)]
pub struct GoogleCaches {
    tokens: Mutex<HashMap<String, (String, Instant)>>,
    folders: Mutex<HashMap<String, String>>,
    /// Serializes folder creation so concurrent runs do not create duplicate folders.
    folder_lock: tokio::sync::Mutex<()>,
}

impl GoogleCaches {
    fn token(&self, key: &str) -> Option<String> {
        let map = self.tokens.lock().unwrap();
        map.get(key).filter(|(_, exp)| *exp > Instant::now()).map(|(t, _)| t.clone())
    }
    fn put_token(&self, key: &str, token: &str, ttl: Duration) {
        self.tokens.lock().unwrap().insert(key.to_string(), (token.to_string(), Instant::now() + ttl));
    }
    fn drop_token(&self, key: &str) {
        self.tokens.lock().unwrap().remove(key);
    }
    fn folder(&self, key: &str) -> Option<String> {
        self.folders.lock().unwrap().get(key).cloned()
    }
    fn put_folder(&self, key: &str, id: &str) {
        self.folders.lock().unwrap().insert(key.to_string(), id.to_string());
    }
}

enum Credentials {
    ServiceAccount { key: ServiceAccountKey, subject: Option<String> },
    OAuth { client_id: String, client_secret: String, refresh_token: String },
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct DriveFile {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default, rename = "md5Checksum")]
    md5_checksum: Option<String>,
    #[serde(default, rename = "webViewLink")]
    web_view_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileList {
    #[serde(default)]
    files: Vec<DriveFile>,
}

struct ApiResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl ApiResponse {
    fn json<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        serde_json::from_slice(&self.body).context("unexpected response from the Google Drive API")
    }
}

enum UploadError {
    SessionExpired,
    Cancelled,
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for UploadError {
    fn from(err: anyhow::Error) -> Self {
        UploadError::Failed(err)
    }
}

enum SessionState {
    Offset(u64),
    Done(String),
    Expired,
}

pub struct GdriveStorage {
    ctx: StorageContext,
    creds: Credentials,
    root: String,
    base_path: Vec<String>,
    subfolders: bool,
    cache_key: String,
}

impl GdriveStorage {
    pub fn new(
        cache_key: String,
        config: &GdriveConfig,
        secret: GdriveSecret,
        ctx: StorageContext,
    ) -> anyhow::Result<Self> {
        let creds = match config.auth {
            GdriveAuthKind::ServiceAccount => {
                let json = secret
                    .service_account_json
                    .ok_or_else(|| anyhow!("no service account key has been uploaded for this destination"))?;
                Credentials::ServiceAccount {
                    key: ServiceAccountKey::parse(&json)?,
                    subject: config.impersonate.clone().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                }
            }
            GdriveAuthKind::Oauth => Credentials::OAuth {
                client_id: config
                    .client_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| anyhow!("the OAuth client ID is missing"))?,
                client_secret: secret.client_secret.ok_or_else(|| anyhow!("the OAuth client secret is missing"))?,
                refresh_token: secret.refresh_token.ok_or_else(|| {
                    anyhow!(
                        "the Google account is not connected yet: use \"Connect Google account\" on the destination"
                    )
                })?,
            },
        };
        let folder = extract_folder_id(&config.folder_id);
        let (root, base_path) = if folder.is_empty() {
            if matches!(creds, Credentials::ServiceAccount { subject: None, .. }) {
                bail!(
                    "a folder ID is required with a service account: create a folder in a Shared Drive \
                     and add the service account as a member"
                );
            }
            ("root".to_string(), vec![DEFAULT_FOLDER_NAME.to_string()])
        } else {
            (folder, Vec::new())
        };
        Ok(Self { ctx, creds, root, base_path, subfolders: config.subfolder_per_database, cache_key })
    }

    fn is_service_account(&self) -> bool {
        matches!(self.creds, Credentials::ServiceAccount { subject: None, .. })
    }

    // ---------------------------------------------------------------- auth

    async fn access_token(&self) -> anyhow::Result<String> {
        if let Some(token) = self.ctx.google.token(&self.cache_key) {
            return Ok(token);
        }
        let response = match &self.creds {
            Credentials::ServiceAccount { key, subject } => {
                let now = chrono::Utc::now().timestamp();
                let claims = JwtClaims {
                    iss: &key.client_email,
                    scope: DRIVE_SCOPE,
                    aud: &self.ctx.google_token_url,
                    iat: now - 30,
                    exp: now + 3600 - 30,
                    sub: subject.as_deref(),
                };
                let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
                header.kid = key.private_key_id.clone();
                let signing_key = jsonwebtoken::EncodingKey::from_rsa_pem(key.private_key.as_bytes())
                    .context("the service account private key is invalid")?;
                let assertion = jsonwebtoken::encode(&header, &claims, &signing_key)
                    .context("cannot sign the service account assertion")?;
                self.token_request(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                    ("assertion", assertion.as_str()),
                ])
                .await?
            }
            Credentials::OAuth { client_id, client_secret, refresh_token } => {
                self.token_request(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id.as_str()),
                    ("client_secret", client_secret.as_str()),
                    ("refresh_token", refresh_token.as_str()),
                ])
                .await?
            }
        };
        let ttl = response.expires_in.unwrap_or(3600).saturating_sub(300).max(60);
        self.ctx.google.put_token(&self.cache_key, &response.access_token, Duration::from_secs(ttl));
        Ok(response.access_token)
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> anyhow::Result<TokenResponse> {
        let mut attempt = 0;
        loop {
            match self.ctx.http.post(&self.ctx.google_token_url).form(form).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json().await.context("invalid answer from the Google token endpoint");
                }
                Ok(resp) if resp.status().is_server_error() && attempt < 3 => {}
                Ok(resp) => {
                    let status = resp.status();
                    let body: Value = resp.json().await.unwrap_or_default();
                    let error = body["error"].as_str().unwrap_or("unknown_error");
                    let description = body["error_description"].as_str().unwrap_or("");
                    let hint = match (error, &self.creds) {
                        ("invalid_grant", Credentials::OAuth { .. }) => {
                            " The refresh token was revoked or has expired: reconnect the Google account. \
                             OAuth consent screens left in \"Testing\" mode issue tokens that expire after \
                             7 days; publish the app (\"In production\") to avoid this."
                        }
                        ("invalid_grant", _) => {
                            " Check that the key has not been deleted in Google Cloud and that the server clock is correct."
                        }
                        ("unauthorized_client", Credentials::ServiceAccount { subject: Some(_), .. }) => {
                            " Domain-wide delegation is not authorized for this service account and the Drive scope."
                        }
                        ("invalid_client", _) => " The OAuth client ID or secret is wrong.",
                        _ => "",
                    };
                    bail!("Google authentication failed (HTTP {status}): {error} {description}{hint}");
                }
                Err(err) if attempt < 3 => {
                    tracing::warn!(error = %error_chain(&err), "Google token request failed, retrying");
                }
                Err(err) => bail!("cannot reach the Google token endpoint: {}", error_chain(&err)),
            }
            attempt += 1;
            tokio::time::sleep(backoff(attempt, Duration::from_secs(1), Duration::from_secs(10))).await;
        }
    }

    // ---------------------------------------------------------------- API helpers

    /// Sends a small API request, retrying transient failures and refreshing the token once.
    /// Returns the (buffered) response for any non-transient status.
    async fn api(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, &str)],
        headers: &[(&'static str, String)],
        body: Option<&Value>,
    ) -> anyhow::Result<ApiResponse> {
        let mut attempt = 0u32;
        let mut refreshed = false;
        loop {
            let token = self.access_token().await?;
            let mut request = self.ctx.http.request(method.clone(), url).bearer_auth(&token).query(query);
            for (name, value) in headers {
                request = request.header(*name, value);
            }
            if let Some(body) = body {
                request = request.json(body);
            }
            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let headers = resp.headers().clone();
                    let body = resp.bytes().await.unwrap_or_default();
                    if status == StatusCode::UNAUTHORIZED && !refreshed {
                        self.ctx.google.drop_token(&self.cache_key);
                        refreshed = true;
                        continue;
                    }
                    let transient = status == StatusCode::TOO_MANY_REQUESTS
                        || status.is_server_error()
                        || (status == StatusCode::FORBIDDEN && is_rate_limited(&body));
                    if transient && attempt < MAX_API_ATTEMPTS {
                        attempt += 1;
                        tokio::time::sleep(backoff(attempt, Duration::from_secs(1), Duration::from_secs(32))).await;
                        continue;
                    }
                    return Ok(ApiResponse { status, headers, body });
                }
                Err(err) if attempt < MAX_API_ATTEMPTS => {
                    tracing::warn!(error = %error_chain(&err), "Google Drive request failed, retrying");
                    attempt += 1;
                    tokio::time::sleep(backoff(attempt, Duration::from_secs(1), Duration::from_secs(32))).await;
                }
                Err(err) => bail!("cannot reach the Google Drive API: {}", error_chain(&err)),
            }
        }
    }

    async fn api_ok(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> anyhow::Result<ApiResponse> {
        let resp = self.api(method, url, query, &[], body).await?;
        if resp.status.is_success() { Ok(resp) } else { Err(self.google_error(resp.status, &resp.body)) }
    }

    fn google_error(&self, status: StatusCode, body: &[u8]) -> anyhow::Error {
        let value: Value = serde_json::from_slice(body).unwrap_or_default();
        let message = value["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(300).collect());
        let reason = value["error"]["errors"][0]["reason"].as_str().unwrap_or("");
        let hint = match reason {
            "storageQuotaExceeded" if self.is_service_account() => {
                " Service accounts have no Google Drive storage of their own: use a folder inside a \
                 Shared Drive (add the service account as Content manager), or use OAuth with a regular \
                 Google account."
            }
            "storageQuotaExceeded" => " The Google Drive storage quota of this account is full.",
            "notFound" => " Check the folder ID and that the folder is shared with this account.",
            "insufficientFilePermissions" | "forbidden" => {
                " The account needs write access to the folder (Editor or Content manager)."
            }
            _ => "",
        };
        let reason = if reason.is_empty() { String::new() } else { format!(", {reason}") };
        anyhow!("Google Drive error (HTTP {}{reason}): {message}{hint}", status.as_u16())
    }

    fn files_url(&self) -> String {
        format!("{}/drive/v3/files", self.ctx.google_api_base)
    }

    // ---------------------------------------------------------------- folders

    async fn resolve_folder(&self, extra: &[String]) -> anyhow::Result<String> {
        let path: Vec<&String> = self.base_path.iter().chain(extra.iter()).collect();
        if path.is_empty() {
            return Ok(self.root.clone());
        }
        let _guard = self.ctx.google.folder_lock.lock().await;
        let mut parent = self.root.clone();
        for name in path {
            let key = format!("{}|{}|{}", self.cache_key, parent, name);
            if let Some(id) = self.ctx.google.folder(&key) {
                parent = id;
                continue;
            }
            let id = match self.find_folder(&parent, name).await? {
                Some(id) => id,
                None => self.create_folder(&parent, name).await?,
            };
            self.ctx.google.put_folder(&key, &id);
            parent = id;
        }
        Ok(parent)
    }

    async fn find_folder(&self, parent: &str, name: &str) -> anyhow::Result<Option<String>> {
        let q = format!(
            "name = '{}' and '{}' in parents and mimeType = '{FOLDER_MIME}' and trashed = false",
            escape_query(name),
            escape_query(parent)
        );
        let resp = self
            .api_ok(
                Method::GET,
                &self.files_url(),
                &[
                    ("q", q.as_str()),
                    ("fields", "files(id,name)"),
                    ("pageSize", "10"),
                    ("supportsAllDrives", "true"),
                    ("includeItemsFromAllDrives", "true"),
                    ("corpora", "allDrives"),
                ],
                None,
            )
            .await?;
        let list: FileList = resp.json()?;
        Ok(list.files.into_iter().next().map(|f| f.id))
    }

    async fn create_folder(&self, parent: &str, name: &str) -> anyhow::Result<String> {
        let body = json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] });
        let resp = self
            .api_ok(Method::POST, &self.files_url(), &[("supportsAllDrives", "true"), ("fields", "id")], Some(&body))
            .await
            .with_context(|| format!("creating folder \"{name}\""))?;
        Ok(resp.json::<DriveFile>()?.id)
    }

    // ---------------------------------------------------------------- upload

    pub async fn upload(
        &self,
        req: &UploadRequest<'_>,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> anyhow::Result<StoredObject> {
        let folder: &[String] = if self.subfolders { req.folder } else { &[] };
        let parent = self.resolve_folder(folder).await?;

        let mut restarts = 0;
        let file_id = loop {
            let session = self.create_session(&parent, req.file_name, req.size, req.mime).await?;
            match self.upload_session(&session, req, progress.clone(), cancel).await {
                Ok(id) => break id,
                Err(UploadError::SessionExpired) if restarts < MAX_SESSION_RESTARTS => {
                    restarts += 1;
                    tracing::warn!(file = req.file_name, "upload session expired, restarting upload");
                }
                Err(UploadError::SessionExpired) => bail!("the Google Drive upload session kept expiring"),
                Err(UploadError::Cancelled) => {
                    // Tell Drive to discard the partial upload (best effort).
                    let _ = self.ctx.http.delete(&session).send().await;
                    return Err(Cancelled.into());
                }
                Err(UploadError::Failed(err)) => return Err(err),
            }
        };

        let meta = self.file_metadata(&file_id).await?;
        let remote_size = meta.size.as_deref().and_then(|s| s.parse::<u64>().ok());
        if remote_size.is_some_and(|s| s != req.size) {
            let _ = self.delete(&file_id).await;
            bail!(
                "size mismatch after upload: local {} bytes, Google Drive {} bytes",
                req.size,
                remote_size.unwrap_or_default()
            );
        }
        if let Some(remote_md5) = &meta.md5_checksum
            && !remote_md5.eq_ignore_ascii_case(&req.hashes.md5)
        {
            let _ = self.delete(&file_id).await;
            bail!("checksum mismatch after upload: local MD5 {}, Google Drive MD5 {remote_md5}", req.hashes.md5);
        }

        let mut display = vec!["Google Drive".to_string()];
        display.extend(self.base_path.iter().cloned());
        display.extend(folder.iter().cloned());
        display.push(meta.name.clone().unwrap_or_else(|| req.file_name.to_string()));
        Ok(StoredObject {
            remote_id: file_id,
            remote_path: display.join(" / "),
            web_url: meta.web_view_link,
            size: req.size,
        })
    }

    async fn create_session(&self, parent: &str, name: &str, size: u64, mime: &str) -> anyhow::Result<String> {
        let url = format!("{}/upload/drive/v3/files", self.ctx.google_api_base);
        let body = json!({
            "name": name,
            "parents": [parent],
            "description": "Odoo backup created by odoo-backup",
        });
        let resp = self
            .api(
                Method::POST,
                &url,
                &[("uploadType", "resumable"), ("supportsAllDrives", "true")],
                &[("X-Upload-Content-Type", mime.to_string()), ("X-Upload-Content-Length", size.to_string())],
                Some(&body),
            )
            .await?;
        if !resp.status.is_success() {
            return Err(self.google_error(resp.status, &resp.body));
        }
        resp.headers
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Google Drive did not return an upload session URL"))
    }

    async fn upload_session(
        &self,
        session: &str,
        req: &UploadRequest<'_>,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> Result<String, UploadError> {
        let size = req.size;
        let chunk_size = self.ctx.chunk_size as u64;
        let mut file = tokio::fs::File::open(req.file).await.map_err(anyhow::Error::from)?;
        let mut offset: u64 = 0;
        let mut failures: u32 = 0;
        let mut cached: Option<(u64, Bytes)> = None;

        loop {
            if cancel.is_cancelled() {
                return Err(UploadError::Cancelled);
            }

            let len = chunk_size.min(size - offset);
            let data = match &cached {
                Some((at, data)) if *at == offset && data.len() as u64 == len => data.clone(),
                _ => {
                    let mut buf = vec![0u8; len as usize];
                    file.seek(std::io::SeekFrom::Start(offset)).await.map_err(anyhow::Error::from)?;
                    file.read_exact(&mut buf).await.map_err(anyhow::Error::from)?;
                    let data = Bytes::from(buf);
                    cached = Some((offset, data.clone()));
                    data
                }
            };

            let range = if size == 0 {
                "bytes */0".to_string()
            } else {
                format!("bytes {}-{}/{}", offset, offset + len - 1, size)
            };
            let request = self
                .ctx
                .http
                .put(session)
                .header(CONTENT_LENGTH, len)
                .header(CONTENT_RANGE, range)
                .body(progress_body(data, offset, progress.clone()))
                .send();
            let result = tokio::select! {
                r = request => r,
                _ = cancel.cancelled() => return Err(UploadError::Cancelled),
            };

            let problem = match result {
                Ok(resp) => match resp.status().as_u16() {
                    200 | 201 => {
                        let file: DriveFile = resp.json().await.map_err(|e| anyhow!("invalid upload response: {e}"))?;
                        progress(size);
                        return Ok(file.id);
                    }
                    308 => {
                        let next = next_offset(resp.headers());
                        if next < offset {
                            tracing::warn!(offset, next, "Drive acknowledged less data than sent, rewinding");
                        }
                        offset = next;
                        failures = 0;
                        progress(offset);
                        continue;
                    }
                    404 | 410 => return Err(UploadError::SessionExpired),
                    status if status == 429 || status >= 500 => format!("HTTP {status}"),
                    status => {
                        let body = resp.bytes().await.unwrap_or_default();
                        return Err(UploadError::Failed(
                            self.google_error(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST), &body),
                        ));
                    }
                },
                Err(err) => error_chain(&err),
            };

            // Transient failure: wait, ask Drive how much it received, and resume from there.
            loop {
                failures += 1;
                if failures > MAX_CHUNK_FAILURES {
                    return Err(UploadError::Failed(anyhow!(
                        "upload to Google Drive failed after {} retries at {} of {}: {problem}",
                        MAX_CHUNK_FAILURES,
                        format_bytes(offset),
                        format_bytes(size)
                    )));
                }
                tracing::warn!(attempt = failures, %problem, "chunk upload failed, resuming");
                let wait = backoff(failures, Duration::from_secs(1), Duration::from_secs(64));
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = cancel.cancelled() => return Err(UploadError::Cancelled),
                }
                match self.session_state(session, size).await {
                    Ok(SessionState::Offset(next)) => {
                        offset = next;
                        progress(offset);
                        break;
                    }
                    Ok(SessionState::Done(id)) => {
                        progress(size);
                        return Ok(id);
                    }
                    Ok(SessionState::Expired) => return Err(UploadError::SessionExpired),
                    Err(err) => {
                        tracing::warn!(error = %err, "cannot query upload status");
                    }
                }
            }
        }
    }

    async fn session_state(&self, session: &str, size: u64) -> anyhow::Result<SessionState> {
        let resp = self
            .ctx
            .http
            .put(session)
            .header(CONTENT_LENGTH, 0)
            .header(CONTENT_RANGE, format!("bytes */{size}"))
            .send()
            .await
            .map_err(|e| anyhow!(error_chain(&e)))?;
        match resp.status().as_u16() {
            200 | 201 => Ok(SessionState::Done(resp.json::<DriveFile>().await?.id)),
            308 => Ok(SessionState::Offset(next_offset(resp.headers()))),
            404 | 410 => Ok(SessionState::Expired),
            status => bail!("unexpected HTTP {status} while querying the upload status"),
        }
    }

    async fn file_metadata(&self, file_id: &str) -> anyhow::Result<DriveFile> {
        let url = format!("{}/{}", self.files_url(), file_id);
        let resp = self
            .api_ok(
                Method::GET,
                &url,
                &[("fields", "id,name,size,md5Checksum,webViewLink"), ("supportsAllDrives", "true")],
                None,
            )
            .await?;
        resp.json()
    }

    // ---------------------------------------------------------------- delete / download

    pub async fn delete(&self, file_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/{}", self.files_url(), file_id);
        let resp = self.api(Method::DELETE, &url, &[("supportsAllDrives", "true")], &[], None).await?;
        match resp.status.as_u16() {
            200..=299 | 404 => Ok(()),
            // Content managers of a Shared Drive may trash but not permanently delete.
            403 => {
                let body = json!({ "trashed": true });
                self.api_ok(Method::PATCH, &url, &[("supportsAllDrives", "true")], Some(&body))
                    .await
                    .context("moving the file to the trash")?;
                Ok(())
            }
            _ => Err(self.google_error(resp.status, &resp.body)),
        }
    }

    pub async fn open_stream(&self, file_id: &str) -> anyhow::Result<(ByteStream, Option<u64>)> {
        let url = format!("{}/{}", self.files_url(), file_id);
        let mut refreshed = false;
        loop {
            let token = self.access_token().await?;
            let resp = self
                .ctx
                .http
                .get(&url)
                .bearer_auth(&token)
                .query(&[("alt", "media"), ("supportsAllDrives", "true"), ("acknowledgeAbuse", "true")])
                .send()
                .await
                .map_err(|e| anyhow!("cannot download from Google Drive: {}", error_chain(&e)))?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED && !refreshed {
                self.ctx.google.drop_token(&self.cache_key);
                refreshed = true;
                continue;
            }
            if !status.is_success() {
                let body = resp.bytes().await.unwrap_or_default();
                return Err(self.google_error(status, &body));
            }
            let length = resp.content_length();
            let stream = resp.bytes_stream().map(|r| r.map_err(std::io::Error::other)).boxed();
            return Ok((stream, length));
        }
    }

    // ---------------------------------------------------------------- test

    pub async fn test(&self) -> TestReport {
        let mut report = TestReport::default();

        match self.access_token().await {
            Ok(_) => {
                let who = match &self.creds {
                    Credentials::ServiceAccount { key, subject: Some(sub) } => {
                        format!("Service account {} impersonating {sub}", key.client_email)
                    }
                    Credentials::ServiceAccount { key, .. } => format!("Service account {}", key.client_email),
                    Credentials::OAuth { .. } => "OAuth refresh token accepted".to_string(),
                };
                report.ok("Authentication", who);
            }
            Err(err) => {
                report.fail("Authentication", format!("{err:#}"));
                return report.finish();
            }
        }

        let about_url = format!("{}/drive/v3/about", self.ctx.google_api_base);
        if let Ok(resp) = self
            .api_ok(Method::GET, &about_url, &[("fields", "user(emailAddress),storageQuota(limit,usage)")], None)
            .await
            && let Ok(about) = resp.json::<Value>()
        {
            let email = about["user"]["emailAddress"].as_str().unwrap_or("unknown account");
            let usage = about["storageQuota"]["usage"].as_str().and_then(|s| s.parse::<u64>().ok());
            let limit = about["storageQuota"]["limit"].as_str().and_then(|s| s.parse::<u64>().ok());
            let detail = match (usage, limit) {
                (Some(u), Some(l)) if l > 0 => {
                    format!("{email}: {} used of {}", format_bytes(u), format_bytes(l))
                }
                (Some(u), _) => format!("{email}: {} used", format_bytes(u)),
                _ => email.to_string(),
            };
            report.ok("Account", detail);
        }

        let mut in_shared_drive = false;
        if self.root != "root" {
            let url = format!("{}/{}", self.files_url(), self.root);
            match self
                .api_ok(
                    Method::GET,
                    &url,
                    &[
                        ("fields", "id,name,mimeType,driveId,capabilities(canAddChildren)"),
                        ("supportsAllDrives", "true"),
                    ],
                    None,
                )
                .await
                .and_then(|r| r.json::<Value>())
            {
                Ok(folder) => {
                    let name = folder["name"].as_str().unwrap_or("?");
                    in_shared_drive = folder["driveId"].is_string();
                    if folder["mimeType"].as_str() != Some(FOLDER_MIME) {
                        report.fail("Target folder", format!("\"{name}\" is not a folder"));
                        return report.finish();
                    }
                    if folder["capabilities"]["canAddChildren"].as_bool() == Some(false) {
                        report.fail("Target folder", format!("no permission to add files to \"{name}\""));
                        return report.finish();
                    }
                    let place = if in_shared_drive { "in a Shared Drive" } else { "in My Drive" };
                    report.ok("Target folder", format!("\"{name}\" ({place}), writable"));
                }
                Err(err) => {
                    report.fail("Target folder", format!("{err:#}"));
                    return report.finish();
                }
            }
        } else {
            report.ok("Target folder", format!("My Drive / {}", self.base_path.join(" / ")));
        }

        if self.is_service_account() && !in_shared_drive {
            report.warn(
                "Storage quota",
                "The folder is not in a Shared Drive. Service accounts have no storage quota, so uploads \
                 to a regular My Drive folder fail with storageQuotaExceeded.",
            );
        }

        match self.write_test().await {
            Ok(()) => report.ok("Write test", "uploaded and deleted a small test file using a resumable session"),
            Err(err) => report.fail("Write test", format!("{err:#}")),
        }
        report.finish()
    }

    async fn write_test(&self) -> anyhow::Result<()> {
        let parent = self.resolve_folder(&[]).await?;
        let payload = Bytes::from_static(b"odoo-backup write test - safe to delete\n");
        let name = format!(".odoo-backup-write-test-{}.txt", chrono::Utc::now().format("%Y%m%d%H%M%S"));
        let session = self.create_session(&parent, &name, payload.len() as u64, "text/plain").await?;
        let resp = self
            .ctx
            .http
            .put(&session)
            .header(CONTENT_RANGE, format!("bytes 0-{}/{}", payload.len() - 1, payload.len()))
            .body(payload)
            .send()
            .await
            .map_err(|e| anyhow!(error_chain(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.bytes().await.unwrap_or_default();
            return Err(self.google_error(status, &body));
        }
        let file: DriveFile = resp.json().await?;
        self.delete(&file.id).await
    }
}

// -------------------------------------------------------------------- OAuth helpers

pub fn authorize_url(auth_url: &str, client_id: &str, redirect_uri: &str, state: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(auth_url)?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", DRIVE_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true")
        .append_pair("state", state);
    Ok(url.to_string())
}

pub struct OAuthTokens {
    pub refresh_token: String,
    pub access_token: String,
}

pub async fn exchange_code(
    ctx: &StorageContext,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<OAuthTokens> {
    let resp = ctx
        .http
        .post(&ctx.google_token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| anyhow!("cannot reach Google: {}", error_chain(&e)))?;
    if !resp.status().is_success() {
        let body: Value = resp.json().await.unwrap_or_default();
        bail!(
            "Google rejected the authorization code: {} {}",
            body["error"].as_str().unwrap_or(""),
            body["error_description"].as_str().unwrap_or("")
        );
    }
    let tokens: TokenResponse = resp.json().await?;
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        anyhow!("Google did not return a refresh token. Remove the app's access at myaccount.google.com/permissions and connect again.")
    })?;
    Ok(OAuthTokens { refresh_token, access_token: tokens.access_token })
}

pub async fn account_email(ctx: &StorageContext, access_token: &str) -> Option<String> {
    let resp = ctx
        .http
        .get(format!("{}/drive/v3/about", ctx.google_api_base))
        .bearer_auth(access_token)
        .query(&[("fields", "user(emailAddress)")])
        .send()
        .await
        .ok()?;
    let value: Value = resp.json().await.ok()?;
    value["user"]["emailAddress"].as_str().map(str::to_string)
}

// -------------------------------------------------------------------- helpers

/// Accepts a raw folder ID or a Drive folder URL.
pub fn extract_folder_id(input: &str) -> String {
    let input = input.trim();
    if let Some(idx) = input.find("/folders/") {
        let rest = &input[idx + "/folders/".len()..];
        return rest.split(['?', '/', '#']).next().unwrap_or("").to_string();
    }
    if let Some(idx) = input.find("id=") {
        return input[idx + 3..].split('&').next().unwrap_or("").to_string();
    }
    input.to_string()
}

fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn is_rate_limited(body: &[u8]) -> bool {
    let value: Value = serde_json::from_slice(body).unwrap_or_default();
    matches!(value["error"]["errors"][0]["reason"].as_str(), Some("rateLimitExceeded" | "userRateLimitExceeded"))
}

/// Parses the `Range: bytes=0-N` header of a 308 answer into the next offset to send.
fn next_offset(headers: &HeaderMap) -> u64 {
    headers
        .get(RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('-').next())
        .and_then(|end| end.trim().parse::<u64>().ok())
        .map(|end| end + 1)
        .unwrap_or(0)
}

/// Request body that reports progress as hyper pulls 256 KiB slices of the chunk.
fn progress_body(data: Bytes, base: u64, progress: ProgressFn) -> reqwest::Body {
    const PIECE: usize = 256 * 1024;
    let total = data.len();
    let pieces: Vec<Bytes> =
        (0..total).step_by(PIECE).map(|start| data.slice(start..(start + PIECE).min(total))).collect();
    let mut sent = base;
    let stream = futures_util::stream::iter(pieces).map(move |piece| {
        sent += piece.len() as u64;
        progress(sent);
        Ok::<Bytes, std::io::Error>(piece)
    });
    reqwest::Body::wrap_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn folder_ids_are_extracted() {
        assert_eq!(extract_folder_id("1AbCdEf"), "1AbCdEf");
        assert_eq!(extract_folder_id("https://drive.google.com/drive/folders/1AbCdEf?usp=sharing"), "1AbCdEf");
        assert_eq!(extract_folder_id("https://drive.google.com/drive/u/0/folders/XYZ_-1"), "XYZ_-1");
        assert_eq!(extract_folder_id("https://drive.google.com/open?id=QQQ&x=1"), "QQQ");
        assert_eq!(extract_folder_id("  "), "");
    }

    #[test]
    fn range_header_parsing() {
        let mut headers = HeaderMap::new();
        assert_eq!(next_offset(&headers), 0);
        headers.insert(RANGE, HeaderValue::from_static("bytes=0-262143"));
        assert_eq!(next_offset(&headers), 262_144);
    }

    #[test]
    fn query_escaping() {
        assert_eq!(escape_query("O'Brien\\db"), "O\\'Brien\\\\db");
    }
}

#[cfg(test)]
#[path = "gdrive_mock_tests.rs"]
mod mock_tests;
