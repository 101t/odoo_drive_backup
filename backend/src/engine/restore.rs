//! Restore pipeline: destination → local file (checksum verified) → Odoo `/web/database/restore`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use tokio_util::sync::CancellationToken;

use super::{EngineInner, Outcome, TempFile};
use crate::db::{self, RestoreOptions, Run, get_artifact, get_destination, get_instance, get_run, status};
use crate::odoo::{OdooClient, OdooError};
use crate::storage::Storage;
use crate::util::{Cancelled, format_bytes, format_duration, format_rate};

pub(super) async fn run(engine: &Arc<EngineInner>, run: &Run, cancel: &CancellationToken) -> anyhow::Result<Outcome> {
    let options: RestoreOptions =
        serde_json::from_str(run.restore_options.as_deref().unwrap_or_default()).context("invalid restore options")?;
    let artifact_id = run.source_artifact_id.ok_or_else(|| anyhow!("no source backup recorded"))?;
    let artifact = get_artifact(&engine.db, artifact_id).await.context("the source backup no longer exists")?;
    if artifact.status != status::ARTIFACT_STORED {
        bail!("the source backup is no longer available on {}", artifact.destination_name);
    }
    let remote_id = artifact.remote_id.clone().ok_or_else(|| anyhow!("the source backup has no location"))?;
    let source_run = get_run(&engine.db, artifact.run_id).await?;
    let destination_id =
        artifact.destination_id.ok_or_else(|| anyhow!("the destination holding this backup was deleted"))?;
    let destination = get_destination(&engine.db, destination_id).await?;
    let storage = Storage::open(&destination, &engine.crypto, &engine.storage)?;

    let instance_id = run.instance_id.ok_or_else(|| anyhow!("the target instance no longer exists"))?;
    let instance = get_instance(&engine.db, instance_id).await?;
    let master_password = match &instance.master_password_enc {
        Some(enc) => engine.crypto.decrypt(enc)?,
        None => bail!("no master password is configured for instance \"{}\"", instance.name),
    };

    // ---- obtain the archive locally
    let (path, _cleanup) = match storage.local_path(&remote_id) {
        Some(path) if path.exists() => {
            engine.log(run.id, "info", format!("Using the stored file {}", path.display())).await;
            (path, None)
        }
        _ => {
            engine.set_phase(run.id, "fetching").await;
            let temp_dir = engine.config.temp_dir();
            tokio::fs::create_dir_all(&temp_dir).await?;
            let temp = temp_dir.join(format!("run-{}-restore.part", run.id));
            let guard = TempFile(temp.clone());
            engine.log(run.id, "info", format!("Downloading the backup from {}…", destination.name)).await;
            let started = Instant::now();
            let progress = engine.tracker(
                run.id,
                "fetching",
                Some(destination.name.clone()),
                source_run.size_bytes.map(|s| s as u64),
            );
            let (hashes, size) = storage.download(&remote_id, &temp, progress, cancel).await?;
            if let Some((algorithm, expected)) =
                hashes.mismatch(source_run.sha256.as_deref(), source_run.md5.as_deref())
            {
                let got = if algorithm == "MD5" { &hashes.md5 } else { &hashes.sha256 };
                bail!("checksum mismatch: the stored backup is corrupted (expected {algorithm} {expected}, got {got})");
            }
            engine
                .log(
                    run.id,
                    "info",
                    format!(
                        "Downloaded {} in {} ({}), checksum verified",
                        format_bytes(size),
                        format_duration(started.elapsed()),
                        format_rate(size, started.elapsed())
                    ),
                )
                .await;
            (temp, Some(guard))
        }
    };

    if let Some(warning) = version_mismatch(source_run.odoo_version.as_deref(), instance.odoo_version.as_deref()) {
        engine.log(run.id, "warn", warning).await;
    }

    // ---- upload to Odoo
    engine.set_phase(run.id, "restoring").await;
    let client = OdooClient::new(
        &instance.url,
        instance.verify_tls,
        Duration::from_secs(instance.timeout_secs.clamp(60, 86_400) as u64),
    )?;
    let size = tokio::fs::metadata(&path).await?.len();
    engine
        .log(
            run.id,
            "info",
            format!(
                "Restoring {} into {} as new database \"{}\" ({}{})…",
                format_bytes(size),
                client.base_url(),
                run.database,
                if options.copy { "as a copy: new database UUID" } else { "keeping the database UUID" },
                if options.neutralize { ", neutralized" } else { "" }
            ),
        )
        .await;
    let started = Instant::now();
    let progress = engine.tracker(run.id, "restoring", None, Some(size));
    client
        .restore_from_file(&master_password, &run.database, &path, options.copy, options.neutralize, progress, cancel)
        .await
        .map_err(|err| match err {
            OdooError::Cancelled => anyhow::Error::from(Cancelled),
            other => anyhow::Error::from(other),
        })?;
    engine
        .log(
            run.id,
            "info",
            format!("Database \"{}\" restored in {}", run.database, format_duration(started.elapsed())),
        )
        .await;

    // Refresh the cached database list of the instance (best effort).
    if let Ok(list) = client.list_databases().await {
        let _ = db::query("UPDATE instances SET databases_json = ? WHERE id = ?")
            .bind(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
            .bind(instance.id)
            .execute(&engine.db)
            .await;
    }
    Ok(Outcome::Success)
}

/// Major version of an Odoo version string: "17.0", "saas~17.2", "17.0+e" → "17".
fn major(version: &str) -> Option<&str> {
    let version = version.trim().trim_start_matches("saas~");
    let major = version.split(['.', '+', '-']).next()?;
    (!major.is_empty() && major.chars().all(|c| c.is_ascii_digit())).then_some(major)
}

/// Odoo only restores databases of its own major version (an upgrade needs the upgrade service).
pub fn version_mismatch(backup: Option<&str>, target: Option<&str>) -> Option<String> {
    let (backup, target) = (backup?, target?);
    if major(backup)? == major(target)? {
        return None;
    }
    Some(format!(
        "The backup was made with Odoo {backup} but the target runs Odoo {target}: \
         the restored database may not load without a migration"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_versions() {
        assert_eq!(major("17.0"), Some("17"));
        assert_eq!(major("saas~17.2"), Some("17"));
        assert_eq!(major("16.0+e"), Some("16"));
        assert_eq!(major("unknown"), None);
        assert!(version_mismatch(Some("17.0"), Some("saas~17.4")).is_none());
        assert!(version_mismatch(Some("16.0"), Some("17.0")).is_some());
        assert!(version_mismatch(None, Some("17.0")).is_none());
        assert!(version_mismatch(Some("17.0"), Some("garbage")).is_none());
    }
}
