//! Local directory destination (a disk, NFS/SMB mount, ...).

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use futures_util::StreamExt;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use super::{ByteStream, StoredObject, TestReport, UploadRequest};
use crate::util::{Cancelled, ProgressFn, available_space, format_bytes, sanitize_segment};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    /// Absolute directory where backups are written.
    pub path: String,
    #[serde(default = "default_true")]
    pub subfolder_per_database: bool,
}

pub struct LocalStorage {
    root: PathBuf,
    subfolders: bool,
}

impl LocalStorage {
    pub fn new(config: &LocalConfig) -> anyhow::Result<Self> {
        let root = PathBuf::from(config.path.trim());
        if !root.is_absolute() {
            bail!("the directory must be an absolute path (got \"{}\")", config.path);
        }
        Ok(Self { root, subfolders: config.subfolder_per_database })
    }

    /// Resolves a stored relative path, refusing anything that escapes the root.
    pub fn path_of(&self, remote_id: &str) -> anyhow::Result<PathBuf> {
        let relative = Path::new(remote_id);
        if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("invalid stored path \"{remote_id}\"");
        }
        Ok(self.root.join(relative))
    }

    pub async fn upload(
        &self,
        req: &UploadRequest<'_>,
        progress: ProgressFn,
        cancel: &CancellationToken,
    ) -> anyhow::Result<StoredObject> {
        let mut relative = PathBuf::new();
        if self.subfolders {
            for segment in req.folder {
                relative.push(sanitize_segment(segment));
            }
        }
        let dir = self.root.join(&relative);
        tokio::fs::create_dir_all(&dir).await.with_context(|| format!("creating directory {}", dir.display()))?;

        let file_name = sanitize_segment(req.file_name);
        let final_path = dir.join(&file_name);
        let partial = dir.join(format!(".{file_name}.partial"));

        let result = copy_with_progress(req.file, &partial, progress, cancel).await;
        let (size, md5) = match result {
            Ok(v) => v,
            Err(err) => {
                let _ = tokio::fs::remove_file(&partial).await;
                return Err(err);
            }
        };
        if size != req.size || !md5.eq_ignore_ascii_case(req.md5) {
            let _ = tokio::fs::remove_file(&partial).await;
            bail!("verification failed after copying to {}", dir.display());
        }
        tokio::fs::rename(&partial, &final_path)
            .await
            .with_context(|| format!("renaming into {}", final_path.display()))?;
        if let Ok(dir_handle) = tokio::fs::File::open(&dir).await {
            let _ = dir_handle.sync_all().await;
        }

        let remote_id = relative
            .join(&file_name)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        Ok(StoredObject { remote_id, remote_path: final_path.display().to_string(), web_url: None, size })
    }

    pub async fn delete(&self, remote_id: &str) -> anyhow::Result<()> {
        let path = self.path_of(remote_id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("deleting {}", path.display())),
        }
    }

    pub async fn open_stream(&self, remote_id: &str) -> anyhow::Result<(ByteStream, Option<u64>)> {
        let path = self.path_of(remote_id)?;
        let file = tokio::fs::File::open(&path).await.with_context(|| format!("opening {}", path.display()))?;
        let length = file.metadata().await.ok().map(|m| m.len());
        Ok((ReaderStream::with_capacity(file, 1 << 20).boxed(), length))
    }

    pub async fn test(&self) -> TestReport {
        let mut report = TestReport::default();
        if let Err(err) = tokio::fs::create_dir_all(&self.root).await {
            report.fail("Directory", format!("cannot create {}: {err}", self.root.display()));
            return report.finish();
        }
        report.ok("Directory", self.root.display().to_string());

        let probe = self.root.join(".odoo-backup-write-test");
        let payload = b"odoo-backup write test";
        let write = async {
            tokio::fs::write(&probe, payload).await?;
            let read = tokio::fs::read(&probe).await?;
            tokio::fs::remove_file(&probe).await?;
            anyhow::ensure!(read == payload, "read back different content");
            anyhow::Ok(())
        };
        match write.await {
            Ok(()) => report.ok("Write test", "created, read back and deleted a test file"),
            Err(err) => report.fail("Write test", format!("{err:#}")),
        }

        match available_space(&self.root) {
            Some(free) if free < 1 << 30 => report.warn("Free space", format!("only {} available", format_bytes(free))),
            Some(free) => report.ok("Free space", format!("{} available", format_bytes(free))),
            None => report.warn("Free space", "unknown"),
        }
        report.finish()
    }
}

async fn copy_with_progress(
    source: &Path,
    dest: &Path,
    progress: ProgressFn,
    cancel: &CancellationToken,
) -> anyhow::Result<(u64, String)> {
    let mut reader = tokio::fs::File::open(source).await?;
    let mut writer = tokio::fs::File::create(dest).await.with_context(|| format!("creating {}", dest.display()))?;
    let mut buf = vec![0u8; 4 << 20];
    let mut hasher = Md5::new();
    let mut copied = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n]).await?;
        copied += n as u64;
        progress(copied);
    }
    writer.flush().await?;
    writer.sync_all().await?;
    Ok((copied, hex::encode(hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::no_progress;

    #[tokio::test]
    async fn upload_download_delete() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.zip");
        let payload = b"PK\x03\x04 test archive";
        tokio::fs::write(&source, payload).await.unwrap();
        let md5 = hex::encode(Md5::digest(payload));

        let storage = LocalStorage::new(&LocalConfig {
            path: dir.path().join("dest").display().to_string(),
            subfolder_per_database: true,
        })
        .unwrap();
        let folder = vec!["Prod / EU".to_string(), "acme".to_string()];
        let req = UploadRequest {
            file: &source,
            size: payload.len() as u64,
            file_name: "acme_2026.zip",
            folder: &folder,
            md5: &md5,
            mime: "application/zip",
        };
        let stored = storage.upload(&req, no_progress(), &CancellationToken::new()).await.unwrap();
        assert_eq!(stored.remote_id, "Prod _ EU/acme/acme_2026.zip");
        assert!(storage.path_of(&stored.remote_id).unwrap().exists());

        storage.delete(&stored.remote_id).await.unwrap();
        assert!(!storage.path_of(&stored.remote_id).unwrap().exists());
        storage.delete(&stored.remote_id).await.unwrap(); // idempotent
    }

    #[test]
    fn path_traversal_is_rejected() {
        let storage =
            LocalStorage::new(&LocalConfig { path: "/backups".into(), subfolder_per_database: true }).unwrap();
        assert!(storage.path_of("../etc/passwd").is_err());
        assert!(storage.path_of("/etc/passwd").is_err());
        assert!(storage.path_of("a/b.zip").is_ok());
    }
}
