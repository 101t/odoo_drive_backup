//! Backup pipeline: Odoo → local temp file → verification → every destination → retention.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use tokio_util::sync::CancellationToken;

use super::verify::VerifyReport;
use super::{EngineInner, Outcome, TempFile, retention, verify};
use crate::db::{self, Artifact, Run, get_destination, get_instance, get_job, run_artifacts, status};
use crate::odoo::{BackupFormat, DownloadStats, OdooClient, OdooError};
use crate::storage::{Hashes, Storage, StoredObject, UploadRequest};
use crate::util::{
    Cancelled, ProgressFn, available_space, format_bytes, format_duration, format_rate, is_cancelled, now, slugify,
};

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
    let _ = db::query("UPDATE runs SET odoo_version = ?, manifest_json = ? WHERE id = ?")
        .bind(&report.odoo_version)
        .bind(serde_json::to_string(&report).ok())
        .bind(run.id)
        .execute(&engine.db)
        .await;
    sanity_check(engine, run, format, stats.size, &report).await;

    // ---- upload to every destination at once: the run takes as long as the slowest one
    engine.set_phase(run.id, "uploading").await;
    let folder = vec![instance.name.clone(), run.database.clone()];
    let hashes = Hashes { md5: stats.md5.clone(), sha256: stats.sha256.clone() };
    let pending: Vec<Artifact> =
        run_artifacts(&engine.db, run.id).await?.into_iter().filter(|a| a.status == status::ARTIFACT_PENDING).collect();
    let label = pending.iter().map(|a| a.destination_name.as_str()).collect::<Vec<_>>().join(", ");
    let overall = engine.tracker(run.id, "uploading", Some(label), Some(stats.size * pending.len() as u64));
    let sent: Arc<Vec<AtomicU64>> = Arc::new(pending.iter().map(|_| AtomicU64::new(0)).collect());
    let source =
        Source { file: &temp_path, size: stats.size, hashes: &hashes, file_name: &file_name, folder: &folder, format };
    let uploads = pending.iter().enumerate().map(|(index, artifact)| {
        let (sent, overall) = (sent.clone(), overall.clone());
        let progress: ProgressFn = Arc::new(move |done| {
            sent[index].store(done, Ordering::Relaxed);
            overall(sent.iter().map(|s| s.load(Ordering::Relaxed)).sum());
        });
        store_one(engine, run, artifact, &source, progress, cancel)
    });
    let mut failed: Vec<String> = Vec::new();
    for (artifact, result) in pending.iter().zip(futures_util::future::join_all(uploads).await) {
        if !result? {
            failed.push(artifact.destination_name.clone());
        }
    }
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    if failed.len() == pending.len() {
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

/// The verified local archive being shipped.
struct Source<'a> {
    file: &'a std::path::Path,
    size: u64,
    hashes: &'a Hashes,
    file_name: &'a str,
    folder: &'a [String],
    format: BackupFormat,
}

/// Uploads to one destination and records the outcome on its artifact.
/// `Ok(false)` is a failed upload (already logged); `Err` only for cancellation and database errors.
async fn store_one(
    engine: &Arc<EngineInner>,
    run: &Run,
    artifact: &Artifact,
    source: &Source<'_>,
    progress: ProgressFn,
    cancel: &CancellationToken,
) -> anyhow::Result<bool> {
    match upload_one(engine, run, artifact, source, progress, cancel).await {
        Ok(object) => {
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
            engine.publish_run(run.id).await;
            Ok(true)
        }
        Err(err) if is_cancelled(&err) || cancel.is_cancelled() => Err(Cancelled.into()),
        Err(err) => {
            let message = format!("{err:#}");
            let _ = db::query("UPDATE artifacts SET status = ?, error = ? WHERE id = ?")
                .bind(status::ARTIFACT_FAILED)
                .bind(&message)
                .bind(artifact.id)
                .execute(&engine.db)
                .await;
            engine.log(run.id, "error", format!("Upload to {} failed: {message}", artifact.destination_name)).await;
            engine.publish_run(run.id).await;
            Ok(false)
        }
    }
}

async fn upload_one(
    engine: &Arc<EngineInner>,
    run: &Run,
    artifact: &Artifact,
    source: &Source<'_>,
    progress: ProgressFn,
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
    let request = UploadRequest {
        file: source.file,
        size: source.size,
        file_name: source.file_name,
        folder: source.folder,
        hashes: source.hashes,
        mime: source.format.mime(),
    };
    let object = storage.upload(&request, progress, cancel).await?;
    engine
        .log(
            run.id,
            "info",
            format!(
                "Uploaded to {} in {} ({}), checksum verified",
                destination.name,
                format_duration(started.elapsed()),
                format_rate(source.size, started.elapsed())
            ),
        )
        .await;
    Ok(object)
}

/// Compares the new archive with the previous successful backup and records a warning when it
/// shrank suspiciously (lost attachments, emptied tables, wrong database). Best effort.
async fn sanity_check(engine: &Arc<EngineInner>, run: &Run, format: BackupFormat, size: u64, report: &VerifyReport) {
    let drop_percent = match crate::api::settings::load(&engine.db).await {
        Ok(settings) => settings.size_drop_warn_percent.clamp(0, 99) as u32,
        Err(_) => return,
    };
    let previous: Option<(Option<i64>, Option<String>)> = db::query_as(
        "SELECT size_bytes, manifest_json FROM runs WHERE instance_id = ? AND database = ? AND kind = 'backup' \
         AND format = ? AND id < ? AND status IN (?, ?) AND size_bytes IS NOT NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(run.instance_id)
    .bind(&run.database)
    .bind(format.as_str())
    .bind(run.id)
    .bind(status::SUCCESS)
    .bind(status::PARTIAL)
    .fetch_optional(&engine.db)
    .await
    .ok()
    .flatten();
    let Some((Some(previous_size), manifest)) = previous else {
        return;
    };
    let previous_report: Option<VerifyReport> = manifest.as_deref().and_then(|m| serde_json::from_str(m).ok());
    let warnings = verify::compare(previous_size as u64, previous_report.as_ref(), size, report, drop_percent);
    if warnings.is_empty() {
        return;
    }
    for warning in &warnings {
        engine.log(run.id, "warn", format!("Sanity check: {warning}")).await;
    }
    let _ = db::query("UPDATE runs SET warning = ? WHERE id = ?")
        .bind(warnings.join("; "))
        .bind(run.id)
        .execute(&engine.db)
        .await;
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
