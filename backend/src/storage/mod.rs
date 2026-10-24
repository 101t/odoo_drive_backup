//! Backup destinations: Google Drive and local/mounted directories.

pub mod gdrive;
pub mod local;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use serde::Serialize;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;

use crate::crypto::Crypto;
use crate::db::DestinationRow;
use crate::util::{Cancelled, ProgressFn};

pub use gdrive::{GdriveAuthKind, GdriveConfig, GdriveSecret, GoogleCaches};
pub use local::LocalConfig;

pub type ByteStream = BoxStream<'static, std::io::Result<Bytes>>;

/// Shared resources needed to talk to remote storage.
#[derive(Clone)]
pub struct StorageContext {
    pub http: reqwest::Client,
    pub google: Arc<GoogleCaches>,
    pub google_api_base: String,
    pub google_token_url: String,
    pub chunk_size: usize,
}

impl StorageContext {
    pub fn new(google_api_base: String, google_token_url: String, chunk_size: usize) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("odoo-backup/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(300))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            // Drive answers 308 "Resume Incomplete" during uploads: never treat it as a redirect.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            http,
            google: Arc::new(GoogleCaches::default()),
            google_api_base: google_api_base.trim_end_matches('/').to_string(),
            google_token_url,
            chunk_size,
        })
    }
}

/// Digests of a local file, computed in one pass. Each destination verifies the one it supports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hashes {
    pub md5: String,
    pub sha256: String,
}

impl Hashes {
    /// Compares with recorded digests (hex, any case), preferring SHA-256. Returns the
    /// mismatching algorithm and the expected value, or `None` when everything known matches.
    pub fn mismatch(&self, sha256: Option<&str>, md5: Option<&str>) -> Option<(&'static str, String)> {
        if let Some(expected) = sha256.filter(|s| !s.is_empty()) {
            return (!expected.eq_ignore_ascii_case(&self.sha256)).then(|| ("SHA-256", expected.to_string()));
        }
        md5.filter(|m| !m.is_empty() && !m.eq_ignore_ascii_case(&self.md5)).map(|m| ("MD5", m.to_string()))
    }
}

/// Digest a destination computes on its side and can be checked against [`Hashes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HashKind {
    Md5,
    Sha256,
}

/// Physical kind of storage, used to count "media types" in the 3-2-1 rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    Disk,
    Cloud,
}

/// What a destination can do, so callers adapt without matching on its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub hash: HashKind,
    pub medium: Medium,
    /// Stored away from the Odoo server (a cloud, or a directory mounted from another site).
    pub off_site: bool,
    /// Objects cannot be altered or deleted before their retention date (object lock, WORM).
    pub immutable: bool,
    pub can_delete: bool,
}

/// Capabilities from a destination's kind and public configuration (no secret needed).
pub fn capabilities(kind: &str, config_json: &str) -> Capabilities {
    match kind {
        "gdrive" => Capabilities {
            hash: HashKind::Md5,
            medium: Medium::Cloud,
            off_site: true,
            immutable: false,
            can_delete: true,
        },
        _ => Capabilities {
            hash: HashKind::Sha256,
            medium: Medium::Disk,
            off_site: serde_json::from_str::<LocalConfig>(config_json).is_ok_and(|c| c.off_site),
            immutable: false,
            can_delete: true,
        },
    }
}

pub struct UploadRequest<'a> {
    pub file: &'a Path,
    pub size: u64,
    pub file_name: &'a str,
    /// Sub-folders below the destination root, e.g. `[instance, database]`.
    pub folder: &'a [String],
    pub hashes: &'a Hashes,
    pub mime: &'a str,
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub remote_id: String,
    pub remote_path: String,
    pub web_url: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestStep {
    pub name: String,
    /// `ok`, `warn` or `fail`.
    pub status: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TestReport {
    pub ok: bool,
    pub steps: Vec<TestStep>,
}

impl TestReport {
    pub fn ok(&mut self, name: &str, detail: impl Into<String>) {
        self.steps.push(TestStep { name: name.into(), status: "ok", detail: detail.into() });
    }
    pub fn warn(&mut self, name: &str, detail: impl Into<String>) {
        self.steps.push(TestStep { name: name.into(), status: "warn", detail: detail.into() });
    }
    pub fn fail(&mut self, name: &str, detail: impl Into<String>) {
        self.steps.push(TestStep { name: name.into(), status: "fail", detail: detail.into() });
    }
    pub fn finish(mut self) -> Self {
        self.ok = self.steps.iter().all(|s| s.status != "fail");
        self
    }
}

/// Short-lived handle created per operation, so the variant size difference is irrelevant.
#[allow(clippy::large_enum_variant)]
pub enum Storage {
    Gdrive(gdrive::GdriveStorage),
    Local(local::LocalStorage),
}

impl Storage {
    pub fn open(row: &DestinationRow, crypto: &Crypto, ctx: &StorageContext) -> anyhow::Result<Self> {
        match row.kind.as_str() {
            "gdrive" => {
                let config: GdriveConfig =
                    serde_json::from_str(&row.config_json).context("invalid Google Drive configuration")?;
                let secret: GdriveSecret = match &row.secret_enc {
                    Some(enc) => serde_json::from_str(&crypto.decrypt(enc)?)?,
                    None => GdriveSecret::default(),
                };
                let cache_key = format!("{}:{}", row.id, row.updated_at.timestamp_millis());
                Ok(Storage::Gdrive(gdrive::GdriveStorage::new(cache_key, &config, secret, ctx.clone())?))
            }
            "local" => {
                let config: LocalConfig =
                    serde_json::from_str(&row.config_json).context("invalid local destination configuration")?;
                Ok(Storage::Local(local::LocalStorage::new(&config)?))
            }
            other => bail!("unknown destination type '{other}'"),
        }
    }

    pub async fn upload(
        &self,
        req: &UploadRequest<'_>,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> anyhow::Result<StoredObject> {
        match self {
            Storage::Gdrive(s) => s.upload(req, progress, cancel).await,
            Storage::Local(s) => s.upload(req, progress, cancel).await,
        }
    }

    pub async fn delete(&self, remote_id: &str) -> anyhow::Result<()> {
        match self {
            Storage::Gdrive(s) => s.delete(remote_id).await,
            Storage::Local(s) => s.delete(remote_id).await,
        }
    }

    /// Path of the stored file when it can be read in place (local destinations).
    pub fn local_path(&self, remote_id: &str) -> Option<PathBuf> {
        match self {
            Storage::Local(s) => s.path_of(remote_id).ok(),
            Storage::Gdrive(_) => None,
        }
    }

    pub async fn open_stream(&self, remote_id: &str) -> anyhow::Result<(ByteStream, Option<u64>)> {
        match self {
            Storage::Gdrive(s) => s.open_stream(remote_id).await,
            Storage::Local(s) => s.open_stream(remote_id).await,
        }
    }

    /// Downloads a stored object to `dest`, returning its digests and size.
    pub async fn download(
        &self,
        remote_id: &str,
        dest: &Path,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> anyhow::Result<(Hashes, u64)> {
        use md5::{Digest, Md5};
        use sha2::Sha256;
        let (mut stream, _) = self.open_stream(remote_id).await?;
        let file = tokio::fs::File::create(dest).await?;
        let mut writer = BufWriter::with_capacity(1 << 20, file);
        let mut md5 = Md5::new();
        let mut sha256 = Sha256::new();
        let mut size = 0u64;
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                _ = cancel.cancelled() => return Err(Cancelled.into()),
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| anyhow!("download interrupted: {e}"))?;
            md5.update(&chunk);
            sha256.update(&chunk);
            writer.write_all(&chunk).await?;
            size += chunk.len() as u64;
            progress(size);
        }
        writer.flush().await?;
        writer.into_inner().sync_all().await?;
        let hashes = Hashes { md5: hex::encode(md5.finalize()), sha256: hex::encode(sha256.finalize()) };
        Ok((hashes, size))
    }

    pub async fn test(&self) -> TestReport {
        match self {
            Storage::Gdrive(s) => s.test().await,
            Storage::Local(s) => s.test().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_prefer_sha256() {
        let hashes = Hashes { md5: "aa".into(), sha256: "bb".into() };
        assert_eq!(hashes.mismatch(Some("BB"), Some("zz")), None, "SHA-256 wins over a stale MD5");
        assert_eq!(hashes.mismatch(Some("cc"), Some("aa")), Some(("SHA-256", "cc".into())));
        assert_eq!(hashes.mismatch(None, Some("AA")), None);
        assert_eq!(hashes.mismatch(Some(""), Some("ab")), Some(("MD5", "ab".into())));
        assert_eq!(hashes.mismatch(None, None), None);
    }

    #[test]
    fn capabilities_follow_configuration() {
        let drive = capabilities("gdrive", "{}");
        assert!(drive.off_site && drive.medium == Medium::Cloud && drive.hash == HashKind::Md5);
        let local = capabilities("local", r#"{"path": "/backups"}"#);
        assert!(!local.off_site && local.medium == Medium::Disk);
        assert!(capabilities("local", r#"{"path": "/mnt/nas", "off_site": true}"#).off_site);
    }
}
