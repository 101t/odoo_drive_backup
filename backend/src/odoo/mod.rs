//! Client for Odoo's database-manager HTTP endpoints
//! (`/web/database/backup`, `/web/database/restore`, `/web/database/list`, ...).

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use md5::Md5;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use crate::util::{ProgressFn, error_chain, format_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupFormat {
    /// Database dump plus filestore, as produced by Odoo's database manager.
    Zip,
    /// PostgreSQL custom-format dump only (no filestore).
    Dump,
}

impl BackupFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "zip" => Some(Self::Zip),
            "dump" => Some(Self::Dump),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Dump => "dump",
        }
    }

    pub fn magic(self) -> &'static [u8] {
        match self {
            Self::Zip => b"PK\x03\x04",
            Self::Dump => b"PGDMP",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Zip => "application/zip",
            Self::Dump => "application/octet-stream",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OdooError {
    /// Wrong master password or database management disabled. Retrying will not help.
    #[error("{0}")]
    Access(String),
    /// Odoo answered with an application error (unknown database, ...).
    #[error("{0}")]
    Remote(String),
    /// Network problem, timeout or 5xx. Worth retrying.
    #[error("{0}")]
    Transport(String),
    #[error("cancelled")]
    Cancelled,
    #[error("local I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl OdooError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl From<reqwest::Error> for OdooError {
    fn from(err: reqwest::Error) -> Self {
        let detail = error_chain(&err);
        if err.is_timeout() {
            OdooError::Transport(format!("request to Odoo timed out ({detail})"))
        } else if err.is_connect() {
            OdooError::Transport(format!("cannot connect to Odoo: {detail}"))
        } else {
            OdooError::Transport(detail)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub server_version: String,
    pub server_serie: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadStats {
    pub size: u64,
    pub sha256: String,
    pub md5: String,
}

pub struct OdooClient {
    http: reqwest::Client,
    base: String,
}

impl OdooClient {
    pub fn new(url: &str, verify_tls: bool, timeout: Duration) -> Result<Self, OdooError> {
        let base = normalize_url(url).map_err(OdooError::Remote)?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("odoo-backup/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            // Odoo builds the whole archive before sending the first byte, which can take a
            // long time for big databases: only the idle read timeout is bounded.
            .read_timeout(timeout)
            .tcp_keepalive(Duration::from_secs(30))
            .danger_accept_invalid_certs(!verify_tls)
            // A redirect would turn the POST into a GET and lose the form fields.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| OdooError::Transport(error_chain(&e)))?;
        Ok(Self { http, base })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    async fn json_rpc(&self, path: &str, params: Value) -> Result<Value, OdooError> {
        let resp = self
            .http
            .post(format!("{}{}", self.base, path))
            .json(&json!({ "jsonrpc": "2.0", "method": "call", "params": params, "id": 1 }))
            .send()
            .await?;
        let status = resp.status();
        if status.is_redirection() {
            return Err(redirect_error(&resp));
        }
        if status.as_u16() == 404 {
            return Err(OdooError::Remote(format!(
                "{path} was not found (HTTP 404). Is this an Odoo server, and is the database manager enabled?"
            )));
        }
        if status.is_server_error() {
            return Err(OdooError::Transport(format!("Odoo returned HTTP {status} on {path}")));
        }
        if !status.is_success() {
            return Err(OdooError::Remote(format!("Odoo returned HTTP {status} on {path}")));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| OdooError::Remote(format!("unexpected (non JSON-RPC) answer from {path}: {e}")))?;
        if let Some(err) = body.get("error") {
            return Err(rpc_error(err));
        }
        Ok(body.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn version(&self) -> Result<VersionInfo, OdooError> {
        let result = self.json_rpc("/web/webclient/version_info", json!({})).await?;
        let server_version = result
            .get("server_version")
            .and_then(Value::as_str)
            .ok_or_else(|| OdooError::Remote("version_info did not contain server_version".into()))?
            .to_string();
        let server_serie = result.get("server_serie").and_then(Value::as_str).map(str::to_string);
        Ok(VersionInfo { server_version, server_serie })
    }

    pub async fn list_databases(&self) -> Result<Vec<String>, OdooError> {
        let result = self.json_rpc("/web/database/list", json!({})).await.map_err(|e| match e {
            OdooError::Access(_) => OdooError::Access(
                "database listing is disabled on this Odoo server (list_db = False). \
                     Backups through the database manager need list_db = True."
                    .into(),
            ),
            other => other,
        })?;
        let mut names: Vec<String> =
            serde_json::from_value(result).map_err(|e| OdooError::Remote(format!("unexpected database list: {e}")))?;
        names.sort();
        Ok(names)
    }

    /// Validates the master password without side effects (`db.list_countries` runs `check_super`).
    /// Returns `None` when this Odoo version cannot answer the question.
    pub async fn check_master_password(&self, master_pwd: &str) -> Result<Option<bool>, OdooError> {
        let params = json!({ "service": "db", "method": "list_countries", "args": [master_pwd] });
        match self.json_rpc("/jsonrpc", params).await {
            Ok(_) => Ok(Some(true)),
            Err(OdooError::Access(_)) => Ok(Some(false)),
            Err(OdooError::Remote(msg)) if msg.contains("not found") || msg.contains("KeyError") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Streams `/web/database/backup` into `dest`, hashing on the fly.
    pub async fn backup_to_file(
        &self,
        master_pwd: &str,
        database: &str,
        format: BackupFormat,
        dest: &Path,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> Result<DownloadStats, OdooError> {
        let request = self
            .http
            .post(format!("{}/web/database/backup", self.base))
            .form(&[("master_pwd", master_pwd), ("name", database), ("backup_format", format.as_str())])
            .send();
        let resp = tokio::select! {
            r = request => r?,
            _ = cancel.cancelled() => return Err(OdooError::Cancelled),
        };

        let status = resp.status();
        if status.is_redirection() {
            return Err(redirect_error(&resp));
        }
        if !status.is_success() {
            let hint = if matches!(status.as_u16(), 502 | 504) {
                ". A reverse proxy probably timed out while Odoo was preparing the archive; \
                 raise its read timeout (e.g. nginx proxy_read_timeout) for /web/database/"
            } else {
                ""
            };
            let msg = format!("Odoo returned HTTP {status}{hint}");
            return Err(if status.is_server_error() || status.as_u16() == 429 {
                OdooError::Transport(msg)
            } else {
                OdooError::Remote(msg)
            });
        }

        let content_type =
            resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if content_type.starts_with("text/html") || content_type.starts_with("application/json") {
            let body = resp.text().await.unwrap_or_default();
            return Err(classify_error_page(&body, "backup"));
        }

        let file = tokio::fs::File::create(dest).await?;
        let mut writer = BufWriter::with_capacity(1 << 20, file);
        let mut sha = Sha256::new();
        let mut md5 = Md5::new();
        let mut size: u64 = 0;
        let mut head: Vec<u8> = Vec::with_capacity(16);
        let mut stream = resp.bytes_stream();

        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                _ = cancel.cancelled() => return Err(OdooError::Cancelled),
            };
            match next {
                None => break,
                Some(Ok(chunk)) => {
                    if head.len() < 16 {
                        let take = (16 - head.len()).min(chunk.len());
                        head.extend_from_slice(&chunk[..take]);
                    }
                    sha.update(&chunk);
                    md5.update(&chunk);
                    writer.write_all(&chunk).await?;
                    size += chunk.len() as u64;
                    progress(size);
                }
                Some(Err(e)) => {
                    return Err(OdooError::Transport(format!(
                        "download interrupted after {}: {}",
                        format_bytes(size),
                        error_chain(&e)
                    )));
                }
            }
        }
        writer.flush().await?;
        writer.into_inner().sync_all().await?;

        if size == 0 {
            return Err(OdooError::Remote("Odoo sent an empty backup".into()));
        }
        if !head.starts_with(format.magic()) {
            let preview = String::from_utf8_lossy(&head).to_string();
            if preview.trim_start().starts_with('<') {
                let body = tokio::fs::read_to_string(dest).await.unwrap_or_default();
                return Err(classify_error_page(&body, "backup"));
            }
            return Err(OdooError::Remote(format!(
                "the downloaded file is not a valid .{} backup (unexpected file header)",
                format.as_str()
            )));
        }

        Ok(DownloadStats { size, sha256: hex::encode(sha.finalize()), md5: hex::encode(md5.finalize()) })
    }

    /// Uploads a backup through `/web/database/restore`, creating database `name`.
    #[allow(clippy::too_many_arguments)]
    pub async fn restore_from_file(
        &self,
        master_pwd: &str,
        name: &str,
        file: &Path,
        copy: bool,
        neutralize: bool,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> Result<(), OdooError> {
        let size = tokio::fs::metadata(file).await?.len();
        let reader = tokio::fs::File::open(file).await?;
        let mut sent: u64 = 0;
        let stream = ReaderStream::with_capacity(reader, 1 << 20).map(move |chunk| {
            if let Ok(bytes) = &chunk {
                sent += bytes.len() as u64;
                progress(sent);
            }
            chunk
        });
        let part = Part::stream_with_length(reqwest::Body::wrap_stream(stream), size)
            .file_name("backup.zip")
            .mime_str("application/octet-stream")
            .map_err(|e| OdooError::Transport(e.to_string()))?;
        let mut form = Form::new()
            .text("master_pwd", master_pwd.to_string())
            .text("name", name.to_string())
            .text("copy", if copy { "true" } else { "false" })
            .part("backup_file", part);
        if neutralize {
            form = form.text("neutralize_database", "true");
        }

        let request = self.http.post(format!("{}/web/database/restore", self.base)).multipart(form).send();
        let resp = tokio::select! {
            r = request => r?,
            _ = cancel.cancelled() => return Err(OdooError::Cancelled),
        };
        let status = resp.status();
        // Success is a redirect to the database manager.
        if status.is_redirection() {
            return Ok(());
        }
        if !status.is_success() {
            let msg = format!("Odoo returned HTTP {status} during restore");
            return Err(if status.is_server_error() { OdooError::Transport(msg) } else { OdooError::Remote(msg) });
        }
        let body = resp.text().await.unwrap_or_default();
        Err(classify_error_page(&body, "restore"))
    }
}

/// Normalizes user input such as `https://erp.example.com/web#action=1` to `https://erp.example.com`.
pub fn normalize_url(input: &str) -> Result<String, String> {
    if !input.contains("://") {
        return Err("the URL must start with http:// or https://".into());
    }
    let mut url = url::Url::parse(input.trim()).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("the URL must start with http:// or https://".into());
    }
    if url.host_str().is_none() {
        return Err("the URL has no host".into());
    }
    url.set_query(None);
    url.set_fragment(None);
    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in ["/web/login", "/web/database/manager", "/web/database/selector", "/web", "/odoo"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            path = stripped.to_string();
            break;
        }
    }
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn redirect_error(resp: &reqwest::Response) -> OdooError {
    let target = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("another location");
    OdooError::Remote(format!(
        "Odoo redirected the request to {target}. Use the final address (e.g. https://) as the instance URL."
    ))
}

fn rpc_error(err: &Value) -> OdooError {
    let data = err.get("data");
    let name = data.and_then(|d| d.get("name")).and_then(Value::as_str).unwrap_or("");
    let message = data
        .and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        .or_else(|| err.get("message").and_then(Value::as_str))
        .unwrap_or("Odoo server error")
        .trim()
        .to_string();
    if name.contains("AccessDenied") || message.eq_ignore_ascii_case("access denied") {
        OdooError::Access(message)
    } else {
        OdooError::Remote(if name.is_empty() { message } else { format!("{message} ({name})") })
    }
}

/// Odoo reports database-manager errors by re-rendering the manager page with an alert box.
fn classify_error_page(body: &str, action: &str) -> OdooError {
    let message = extract_alert(body)
        .unwrap_or_else(|| format!("Odoo answered with an HTML page instead of performing the {action}"));
    let lower = message.to_lowercase();
    if lower.contains("access denied") {
        OdooError::Access(format!("{message}. Check the master password (admin_passwd)."))
    } else if lower.contains("database management functions blocked") || lower.contains("list_db") {
        OdooError::Access(format!("{message}. Set list_db = True in the Odoo configuration."))
    } else {
        OdooError::Remote(message)
    }
}

fn extract_alert(body: &str) -> Option<String> {
    let idx = body.find("alert-danger")?;
    let rest = &body[idx..];
    let start = rest.find('>')? + 1;
    let end = rest[start..].find("</div>").map(|e| e + start).unwrap_or(rest.len());
    let text = html_to_text(&rest[start..end]);
    (!text.is_empty()).then_some(text)
}

fn html_to_text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for c in fragment.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decodes the HTML character references Odoo emits (`&quot;`, `&#34;`, `&#x27;`, …).
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .or_else(|| entity.strip_prefix('#').map(str::parse::<u32>))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_normalized() {
        assert_eq!(normalize_url("https://erp.example.com/").unwrap(), "https://erp.example.com");
        assert_eq!(
            normalize_url("https://erp.example.com/web#action=12&model=sale.order").unwrap(),
            "https://erp.example.com"
        );
        assert_eq!(normalize_url("http://10.0.0.5:8069/web/login?db=x").unwrap(), "http://10.0.0.5:8069");
        assert_eq!(normalize_url("https://example.com/odoo").unwrap(), "https://example.com");
        assert!(normalize_url("ftp://example.com").is_err());
        assert!(normalize_url("not a url").is_err());
        assert_eq!(normalize_url("erp.example.com:8069").unwrap_err(), "the URL must start with http:// or https://");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            decode_entities("relation &#34;ir_module&#34; &amp; &#x27;x&#X27; &lt;b&gt;"),
            "relation \"ir_module\" & 'x' <b>"
        );
        assert_eq!(decode_entities("a & b &unknown; &#xZZ; tail&"), "a & b &unknown; &#xZZ; tail&");
    }

    #[test]
    fn error_pages_are_classified() {
        let page = r#"<html><body><div class="alert alert-danger" role="alert">
            Database backup error: Access Denied
        </div></body></html>"#;
        match classify_error_page(page, "backup") {
            OdooError::Access(msg) => assert!(msg.starts_with("Database backup error: Access Denied")),
            other => panic!("unexpected {other:?}"),
        }

        let page =
            r#"<div class="alert alert-danger">Database backup error: database &quot;nope&quot; does not exist</div>"#;
        match classify_error_page(page, "backup") {
            OdooError::Remote(msg) => assert_eq!(msg, r#"Database backup error: database "nope" does not exist"#),
            other => panic!("unexpected {other:?}"),
        }

        assert!(matches!(classify_error_page("<html>login</html>", "backup"), OdooError::Remote(_)));
    }

    #[test]
    fn rpc_errors_are_classified() {
        let err = json!({"code": 200, "message": "Odoo Server Error",
            "data": {"name": "odoo.exceptions.AccessDenied", "message": "Access Denied"}});
        assert!(matches!(rpc_error(&err), OdooError::Access(_)));
        let err = json!({"code": 200, "message": "Odoo Server Error",
            "data": {"name": "builtins.KeyError", "message": "'Method not found: foo'"}});
        assert!(matches!(rpc_error(&err), OdooError::Remote(m) if m.contains("KeyError")));
    }
}
