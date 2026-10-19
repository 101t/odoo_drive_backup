//! Backup pipeline: Odoo → local temp file → verification → every destination → retention.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use tokio_util::sync::CancellationToken;

use super::{EngineInner, Outcome, TempFile, retention, verify};
use crate::db::{self, Artifact, Run, get_destination, get_instance, get_job, run_artifacts, status};
use crate::odoo::{BackupFormat, DownloadStats, OdooClient, OdooError};
use crate::storage::{Storage, StoredObject, UploadRequest};
use crate::util::{Cancelled, available_space, format_bytes, format_duration, format_rate, is_cancelled, now, slugify};

pub(super) async fn run(engine: &Arc<EngineInner>, run: &Run, cancel: &CancellationToken) -> anyhow::Result<Outcome> {
    let instance_id = run.instance_id.ok_or_else(|| anyhow!("the instance no longer exists"))?;
    let instance = get_instance(&engine.db, instance_id).await.context("loading the instance")?;
    let master_password = match &instance.master_password_enc {
        Some(enc) => engine.crypto.decrypt(enc)?,
        None => bail!("no master password is configured for instance \"{}\"", instance.name),
    };
    let job = match run.job_id {
        Some(id) => get_job(&engine.db, id).await.ok(),
        None => None,
    };
    let retries = job.as_ref().map(|j| j.retries.clamp(0, 10) as u32).unwrap_or(1);
    let deep_verify = job.as_ref().map(|j| j.verify_archive).unwrap_or(true);
    let format = BackupFormat::parse(&run.format).unwrap_or(BackupFormat::Zip);

    let file_name = format!(
        "{}_{}_{}.{}",
        slugify(&instance.name),
        crate::util::sanitize_segment(&run.database),
        now().format("%Y-%m-%d_%H-%M-%S"),
        format.as_str()
    );
    db::query("UPDATE runs SET file_name = ? WHERE id = ?").bind(&file_name).bind(run.id).execute(&engine.db).await?;

    let temp_dir = engine.config.temp_dir();
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .with_context(|| format!("creating temp directory {}", temp_dir.display()))?;
    let temp_path = temp_dir.join(format!("run-{}-{}.part", run.id, file_name));
    let _cleanup = TempFile(temp_path.clone());

    check_free_space(engine, run, &temp_dir).await?;

    let client = OdooClient::new(
        &instance.url,
        instance.verify_tls,
        Duration::from_secs(instance.timeout_secs.clamp(60, 86_400) as u64),
    )?;
    engine
        .log(
            run.id,
            "info",
            format!("Requesting a {} backup of \"{}\" from {}", format.as_str(), run.database, client.base_url()),
        )
        .await;

    // ---- download (with retries for transient failures)
    let mut attempt: u32 = 0;
    let stats: DownloadStats = loop {
        attempt += 1;
        let _ = db::query("UPDATE runs SET attempt = ? WHERE id = ?")
            .bind(attempt as i64)
            .bind(run.id)
            .execute(&engine.db)
            .await;
        engine.set_phase(run.id, "downloading").await;
        let started = Instant::now();
        let progress = engine.tracker(run.id, "downloading", None, None);
        match client.backup_to_file(&master_password, &run.database, format, &temp_path, progress, cancel).await {
            Ok(stats) => {
                engine
                    .log(
                        run.id,
                        "info",
                        format!(
                            "Downloaded {} in {} ({})",
                            format_bytes(stats.size),
                            format_duration(started.elapsed()),
                            format_rate(stats.size, started.elapsed())
                        ),
                    )
                    .await;
                break stats;
            }
            Err(OdooError::Cancelled) => return Err(Cancelled.into()),
            Err(err) if err.is_retryable() && attempt <= retries => {
                let wait = Duration::from_secs(30 * attempt as u64);
                engine
                    .log(run.id, "warn", format!("Attempt {attempt} failed: {err}. Retrying in {}s…", wait.as_secs()))
                    .await;
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = cancel.cancelled() => return Err(Cancelled.into()),
                }
            }
            Err(err) => return Err(err.into()),
        }
    };
    db::query("UPDATE runs SET size_bytes = ?, sha256 = ?, md5 = ? WHERE id = ?")
        .bind(stats.size as i64)
        .bind(&stats.sha256)
        .bind(&stats.md5)
        .bind(run.id)
        .execute(&engine.db)
        .await?;
    engine.log(run.id, "info", format!("SHA-256 {}", stats.sha256)).await;

    // ---- verification
    engine.set_phase(run.id, "verifying").await;
    let check_path = temp_path.clone();
    let progress = engine.tracker(run.id, "verifying", None, Some(stats.size));
    let verify_cancel = cancel.clone();
    let report = tokio::task::spawn_blocking(move || {
        verify::verify(&check_path, format, deep_verify, &progress, &verify_cancel)
    })
    .await
    .context("verification task crashed")?
    .context("archive verification failed")?;
    engine.log(run.id, "info", report.summary()).await;
    if let Some(version) = &report.odoo_version {
        let _ = db::query("UPDATE runs SET odoo_version = ? WHERE id = ?")
            .bind(version)
            .bind(run.id)
            .execute(&engine.db)
            .await;
    }

    // ---- upload to every destination
    engine.set_phase(run.id, "uploading").await;
    let folder = vec![instance.name.clone(), run.database.clone()];
    let artifacts = run_artifacts(&engine.db, run.id).await?;
    let mut stored = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for artifact in artifacts.iter().filter(|a| a.status == status::ARTIFACT_PENDING) {
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        match upload_one(engine, run, artifact, &temp_path, &stats, &file_name, &folder, format, cancel).await {
            Ok(object) => {
                stored += 1;
                db::query(
                    "UPDATE artifacts SET status = ?, remote_id = ?, remote_path = ?, web_url = ?, size_bytes = ?, \
                     stored_at = ?, error = NULL WHERE id = ?",
                )
                .bind(status::ARTIFACT_STORED)
                .bind(&object.remote_id)
                .bind(&object.remote_path)
                .bind(&object.web_url)
                .bind(object.size as i64)
                .bind(now())
                .bind(artifact.id)
                .execute(&engine.db)
                .await?;
                engine
                    .log(run.id, "info", format!("Stored on {}: {}", artifact.destination_name, object.remote_path))
                    .await;
            }
            Err(err) if is_cancelled(&err) => return Err(err),
            Err(err) => {
                failed.push(artifact.destination_name.clone());
                let message = format!("{err:#}");
                let _ = db::query("UPDATE artifacts SET status = ?, error = ? WHERE id = ?")
                    .bind(status::ARTIFACT_FAILED)
                    .bind(&message)
                    .bind(artifact.id)
                    .execute(&engine.db)
                    .await;
                engine.log(run.id, "error", format!("Upload to {} failed: {message}", artifact.destination_name)).await;
            }
        }
        engine.publish_run(run.id).await;
    }
    if stored == 0 {
        bail!("the backup could not be stored on any destination");
    }

    // ---- retention
    if let Some(job) = &job {
        engine.set_phase(run.id, "retention").await;
        retention::apply(engine, job, &run.database, run.id).await;
    }

    if failed.is_empty() {
        Ok(Outcome::Success)
    } else {
        Ok(Outcome::Partial(format!("upload failed for {}", failed.join(", "))))
    }
}

#[allow(clippy::too_many_arguments)]
async fn upload_one(
    engine: &Arc<EngineInner>,
    run: &Run,
    artifact: &Artifact,
    file: &std::path::Path,
    stats: &DownloadStats,
    file_name: &str,
    folder: &[String],
    format: BackupFormat,
    cancel: &CancellationToken,
) -> anyhow::Result<StoredObject> {
    let destination_id = artifact.destination_id.ok_or_else(|| anyhow!("the destination has been deleted"))?;
    let destination = get_destination(&engine.db, destination_id).await?;
    let storage = Storage::open(&destination, &engine.crypto, &engine.storage)?;

    db::query("UPDATE artifacts SET status = ? WHERE id = ?")
        .bind(status::ARTIFACT_UPLOADING)
        .bind(artifact.id)
        .execute(&engine.db)
        .await?;
    engine.log(run.id, "info", format!("Uploading to {}…", destination.name)).await;

    let started = Instant::now();
    let progress = engine.tracker(run.id, "uploading", Some(destination.name.clone()), Some(stats.size));
    let request = UploadRequest { file, size: stats.size, file_name, folder, md5: &stats.md5, mime: format.mime() };
    let object = storage.upload(&request, progress, cancel).await?;
    engine
        .log(
            run.id,
            "info",
            format!(
                "Uploaded to {} in {} ({}), checksum verified",
                destination.name,
                format_duration(started.elapsed()),
                format_rate(stats.size, started.elapsed())
            ),
        )
        .await;
    Ok(object)
}

/// Refuses to start when the temp directory obviously cannot hold the backup.
async fn check_free_space(engine: &Arc<EngineInner>, run: &Run, temp_dir: &std::path::Path) -> anyhow::Result<()> {
    let Some(free) = available_space(temp_dir) else {
        return Ok(());
    };
    let previous: Option<i64> = db::query_scalar(
        "SELECT size_bytes FROM runs WHERE instance_id = ? AND database = ? AND kind = 'backup' \
         AND status IN (?, ?) AND size_bytes IS NOT NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(run.instance_id)
    .bind(&run.database)
    .bind(status::SUCCESS)
    .bind(status::PARTIAL)
    .fetch_optional(&engine.db)
    .await?
    .flatten();
    if let Some(previous) = previous {
        let needed = (previous as f64 * 1.2) as u64;
        if free < needed {
            bail!(
                "not enough free space in {}: {} available, about {} needed (last backup was {})",
                temp_dir.display(),
                format_bytes(free),
                format_bytes(needed),
                format_bytes(previous as u64)
            );
        }
    } else if free < 512 * 1024 * 1024 {
        engine.log(run.id, "warn", format!("Low free space in the temp directory: {}", format_bytes(free))).await;
    }
    Ok(())
}
