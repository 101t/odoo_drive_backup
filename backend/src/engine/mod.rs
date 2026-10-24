//! Executes backup and restore runs: queueing, concurrency limits, cancellation,
//! progress reporting, logging, notifications.

pub mod backup;
pub mod policy;
pub mod restore;
pub mod retention;
pub mod verify;

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::Config;
use crate::crypto::Crypto;
use crate::db::{
    self, Artifact, Db, InstanceRow, RestoreOptions, Run, RunLog, get_instance, get_job, get_run, job_destination_ids,
    run_artifacts, status,
};
use crate::events::{Event, EventBus, Progress};
use crate::notify::Notifier;
use crate::odoo::{BackupFormat, OdooClient};
use crate::storage::StorageContext;
use crate::util::{Cancelled, ProgressFn, is_cancelled, now};

pub enum Outcome {
    Success,
    Partial(String),
}

struct ActiveRun {
    cancel: CancellationToken,
    progress: Option<Progress>,
}

pub struct EngineInner {
    pub db: Db,
    pub config: Arc<Config>,
    pub crypto: Arc<Crypto>,
    pub events: EventBus,
    pub storage: StorageContext,
    pub notifier: Notifier,
    semaphore: Arc<Semaphore>,
    instance_locks: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    active: Mutex<HashMap<i64, ActiveRun>>,
    tasks: TaskTracker,
    shutting_down: AtomicBool,
}

#[derive(Clone)]
pub struct Engine(Arc<EngineInner>);

impl Deref for Engine {
    type Target = EngineInner;
    fn deref(&self) -> &EngineInner {
        &self.0
    }
}

pub struct NewBackup<'a> {
    pub instance: &'a InstanceRow,
    pub database: &'a str,
    pub format: BackupFormat,
    pub job_id: Option<i64>,
    pub destination_ids: &'a [i64],
    pub trigger: &'a str,
}

impl Engine {
    pub fn new(
        db: Db,
        config: Arc<Config>,
        crypto: Arc<Crypto>,
        events: EventBus,
        storage: StorageContext,
        notifier: Notifier,
    ) -> Self {
        let permits = config.max_concurrent_jobs.max(1);
        Self(Arc::new(EngineInner {
            db,
            config,
            crypto,
            events,
            storage,
            notifier,
            semaphore: Arc::new(Semaphore::new(permits)),
            instance_locks: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            tasks: TaskTracker::new(),
            shutting_down: AtomicBool::new(false),
        }))
    }

    /// Marks runs left over by a previous process as failed and removes stale temp files.
    pub async fn recover(&self) -> anyhow::Result<()> {
        let message = "Interrupted: the service stopped while this run was in progress";
        let res =
            db::query("UPDATE runs SET status = ?, phase = NULL, error = ?, finished_at = ? WHERE status IN (?, ?)")
                .bind(status::FAILED)
                .bind(message)
                .bind(now())
                .bind(status::QUEUED)
                .bind(status::RUNNING)
                .execute(&self.db)
                .await?;
        if res.rows_affected() > 0 {
            tracing::warn!(count = res.rows_affected(), "marked interrupted runs as failed");
        }
        db::query("UPDATE artifacts SET status = ?, error = 'Interrupted' WHERE status = ?")
            .bind(status::ARTIFACT_FAILED)
            .bind(status::ARTIFACT_UPLOADING)
            .execute(&self.db)
            .await?;
        db::query("UPDATE artifacts SET status = ?, error = 'Not uploaded: the run was interrupted' WHERE status = ?")
            .bind(status::ARTIFACT_SKIPPED)
            .bind(status::ARTIFACT_PENDING)
            .execute(&self.db)
            .await?;

        let temp_dir = self.config.temp_dir();
        if let Ok(mut entries) = tokio::fs::read_dir(&temp_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("run-") && name.ends_with(".part") {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
        Ok(())
    }

    /// Queues one backup run per database of the job.
    pub async fn trigger_job(&self, job_id: i64, trigger: &str) -> anyhow::Result<Vec<i64>> {
        let job = get_job(&self.db, job_id).await.context("job not found")?;
        let instance = get_instance(&self.db, job.instance_id).await.context("instance not found")?;
        let destination_ids = job_destination_ids(&self.db, job.id).await?;
        let format = BackupFormat::parse(&job.format).unwrap_or(BackupFormat::Zip);

        let databases = if job.all_databases {
            match self.discover_databases(&instance).await {
                Ok(list) if !list.is_empty() => list,
                Ok(_) => {
                    self.record_failed_run(
                        &instance,
                        Some(job.id),
                        &job.format,
                        trigger,
                        "the instance has no databases",
                    )
                    .await?;
                    return Ok(Vec::new());
                }
                Err(err) => {
                    let id = self
                        .record_failed_run(
                            &instance,
                            Some(job.id),
                            &job.format,
                            trigger,
                            &format!("cannot list the databases: {err:#}"),
                        )
                        .await?;
                    return Ok(vec![id]);
                }
            }
        } else {
            job.databases()
        };

        if destination_ids.is_empty() {
            bail!("the job has no destination");
        }

        let mut run_ids = Vec::new();
        for database in &databases {
            let busy: i64 =
                db::query_scalar("SELECT COUNT(*) FROM runs WHERE job_id = ? AND database = ? AND status IN (?, ?)")
                    .bind(job.id)
                    .bind(database)
                    .bind(status::QUEUED)
                    .bind(status::RUNNING)
                    .fetch_one(&self.db)
                    .await?;
            if busy > 0 {
                tracing::warn!(job = %job.name, database, "previous run still active, skipping this occurrence");
                continue;
            }
            let id = self
                .enqueue_backup(NewBackup {
                    instance: &instance,
                    database,
                    format,
                    job_id: Some(job.id),
                    destination_ids: &destination_ids,
                    trigger,
                })
                .await?;
            run_ids.push(id);
        }
        Ok(run_ids)
    }

    async fn discover_databases(&self, instance: &InstanceRow) -> anyhow::Result<Vec<String>> {
        let client = OdooClient::new(&instance.url, instance.verify_tls, Duration::from_secs(60))?;
        let list = client.list_databases().await?;
        db::query("UPDATE instances SET databases_json = ?, last_checked_at = ?, last_check_error = NULL WHERE id = ?")
            .bind(serde_json::to_string(&list)?)
            .bind(now())
            .bind(instance.id)
            .execute(&self.db)
            .await?;
        Ok(list)
    }

    /// Records a run that failed before it could start, so it shows up in history and alerts.
    async fn record_failed_run(
        &self,
        instance: &InstanceRow,
        job_id: Option<i64>,
        format: &str,
        trigger: &str,
        error: &str,
    ) -> anyhow::Result<i64> {
        let ts = now();
        let id: i64 = db::query_scalar(
            "INSERT INTO runs (kind, job_id, instance_id, instance_name, database, format, trigger, status, error, \
             created_at, started_at, finished_at) VALUES ('backup', ?, ?, ?, '*', ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(job_id)
        .bind(instance.id)
        .bind(&instance.name)
        .bind(format)
        .bind(trigger)
        .bind(status::FAILED)
        .bind(error)
        .bind(ts)
        .bind(ts)
        .bind(ts)
        .fetch_one(&self.db)
        .await?;
        self.0.log(id, "error", error).await;
        self.0.publish_run(id).await;
        let run = get_run(&self.db, id).await?;
        for (channel, result) in self.notifier.run_finished(&run, &[]).await {
            if let Err(err) = result {
                self.0.log(id, "warn", format!("Notification via {channel} failed: {err:#}")).await;
            }
        }
        Ok(id)
    }

    pub async fn enqueue_backup(&self, spec: NewBackup<'_>) -> anyhow::Result<i64> {
        let ts = now();
        let mut tx = self.db.begin().await?;
        let run_id: i64 = db::query_scalar(
            "INSERT INTO runs (kind, job_id, instance_id, instance_name, database, format, trigger, status, created_at) \
             VALUES ('backup', ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(spec.job_id)
        .bind(spec.instance.id)
        .bind(&spec.instance.name)
        .bind(spec.database)
        .bind(spec.format.as_str())
        .bind(spec.trigger)
        .bind(status::QUEUED)
        .bind(ts)
        .fetch_one(&mut tx)
        .await?;
        for destination_id in spec.destination_ids {
            let (name, kind): (String, String) = db::query_as("SELECT name, kind FROM destinations WHERE id = ?")
                .bind(destination_id)
                .fetch_one(&mut tx)
                .await
                .with_context(|| format!("destination #{destination_id} not found"))?;
            db::query(
                "INSERT INTO artifacts (run_id, destination_id, destination_name, destination_kind, status, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run_id)
            .bind(destination_id)
            .bind(name)
            .bind(kind)
            .bind(status::ARTIFACT_PENDING)
            .bind(ts)
            .execute(&mut tx)
            .await?;
        }
        tx.commit().await?;
        self.0.log(run_id, "info", format!("Queued ({})", spec.trigger)).await;
        self.0.publish_run(run_id).await;
        self.spawn(run_id);
        Ok(run_id)
    }

    pub async fn enqueue_restore(
        &self,
        artifact: &Artifact,
        source: &Run,
        target: &InstanceRow,
        database: &str,
        copy: bool,
        neutralize: bool,
    ) -> anyhow::Result<i64> {
        if artifact.status != status::ARTIFACT_STORED {
            bail!("this copy of the backup is not available (status: {})", artifact.status);
        }
        let options = RestoreOptions {
            copy,
            neutralize,
            source_run_id: source.id,
            source_instance: source.instance_name.clone(),
            source_database: source.database.clone(),
        };
        let run_id: i64 = db::query_scalar(
            "INSERT INTO runs (kind, instance_id, instance_name, database, format, trigger, status, file_name, \
             size_bytes, md5, sha256, odoo_version, source_artifact_id, restore_options, created_at) \
             VALUES ('restore', ?, ?, ?, ?, 'manual', ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(target.id)
        .bind(&target.name)
        .bind(database)
        .bind(&source.format)
        .bind(status::QUEUED)
        .bind(&source.file_name)
        .bind(source.size_bytes)
        .bind(&source.md5)
        .bind(&source.sha256)
        .bind(&source.odoo_version)
        .bind(artifact.id)
        .bind(serde_json::to_string(&options)?)
        .bind(now())
        .fetch_one(&self.db)
        .await?;
        self.0
            .log(
                run_id,
                "info",
                format!(
                    "Queued restore of {} / {} (run #{}) from {}",
                    source.instance_name, source.database, source.id, artifact.destination_name
                ),
            )
            .await;
        self.0.publish_run(run_id).await;
        self.spawn(run_id);
        Ok(run_id)
    }

    fn spawn(&self, run_id: i64) {
        let cancel = CancellationToken::new();
        self.active.lock().unwrap().insert(run_id, ActiveRun { cancel: cancel.clone(), progress: None });
        let inner = self.0.clone();
        self.tasks.spawn(async move { inner.execute(run_id, cancel).await });
    }

    pub fn cancel(&self, run_id: i64) -> bool {
        match self.active.lock().unwrap().get(&run_id) {
            Some(active) => {
                active.cancel.cancel();
                true
            }
            None => false,
        }
    }

    pub fn progress(&self, run_id: i64) -> Option<Progress> {
        self.active.lock().unwrap().get(&run_id).and_then(|a| a.progress.clone())
    }

    pub fn active_count(&self) -> usize {
        self.active.lock().unwrap().len()
    }

    /// Cancels running work and waits (bounded) for it to wind down.
    pub async fn shutdown(&self, timeout: Duration) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for active in self.active.lock().unwrap().values() {
            active.cancel.cancel();
        }
        self.tasks.close();
        if tokio::time::timeout(timeout, self.tasks.wait()).await.is_err() {
            tracing::warn!("some runs did not stop in time");
        }
    }
}

impl EngineInner {
    async fn execute(self: Arc<Self>, run_id: i64, cancel: CancellationToken) {
        let started = Instant::now();
        let result = self.clone().execute_inner(run_id, &cancel).await;
        let (final_status, error) = match &result {
            Ok(Outcome::Success) => (status::SUCCESS, None),
            Ok(Outcome::Partial(msg)) => (status::PARTIAL, Some(msg.clone())),
            Err(_) if cancel.is_cancelled() && self.shutting_down.load(Ordering::SeqCst) => (
                status::FAILED,
                Some("Interrupted: the service was shut down while this run was in progress".to_string()),
            ),
            Err(err) if cancel.is_cancelled() || is_cancelled(err) => {
                (status::CANCELLED, Some("Cancelled by user".to_string()))
            }
            Err(err) => (status::FAILED, Some(format!("{err:#}"))),
        };

        match final_status {
            status::SUCCESS => {
                self.log(
                    run_id,
                    "info",
                    format!("Finished successfully in {}", crate::util::format_duration(started.elapsed())),
                )
                .await
            }
            status::PARTIAL => {
                self.log(run_id, "warn", format!("Finished with problems: {}", error.as_deref().unwrap_or(""))).await
            }
            status::CANCELLED => self.log(run_id, "warn", "Cancelled").await,
            _ => self.log(run_id, "error", format!("Failed: {}", error.as_deref().unwrap_or(""))).await,
        }

        let ts = now();
        let _ = db::query(
            "UPDATE artifacts SET status = ?, error = 'Interrupted during upload' WHERE run_id = ? AND status = ?",
        )
        .bind(status::ARTIFACT_FAILED)
        .bind(run_id)
        .bind(status::ARTIFACT_UPLOADING)
        .execute(&self.db)
        .await;
        let _ = db::query(
            "UPDATE artifacts SET status = ?, error = 'Not uploaded: the run ended before this step' \
             WHERE run_id = ? AND status = ?",
        )
        .bind(status::ARTIFACT_SKIPPED)
        .bind(run_id)
        .bind(status::ARTIFACT_PENDING)
        .execute(&self.db)
        .await;
        if let Err(err) = db::query("UPDATE runs SET status = ?, error = ?, phase = NULL, finished_at = ? WHERE id = ?")
            .bind(final_status)
            .bind(&error)
            .bind(ts)
            .bind(run_id)
            .execute(&self.db)
            .await
        {
            tracing::error!(run_id, error = %err, "cannot store run result");
        }
        self.active.lock().unwrap().remove(&run_id);
        self.publish_run(run_id).await;

        if final_status != status::CANCELLED
            && let Ok(run) = get_run(&self.db, run_id).await
        {
            let artifacts = run_artifacts(&self.db, run_id).await.unwrap_or_default();
            for (channel, result) in self.notifier.run_finished(&run, &artifacts).await {
                match result {
                    Ok(()) => self.log(run_id, "info", format!("Notification sent via {channel}")).await,
                    Err(err) => self.log(run_id, "warn", format!("Notification via {channel} failed: {err:#}")).await,
                }
            }
        }
    }

    async fn execute_inner(self: Arc<Self>, run_id: i64, cancel: &CancellationToken) -> anyhow::Result<Outcome> {
        let run = get_run(&self.db, run_id).await?;
        let instance_id = run.instance_id.ok_or_else(|| anyhow!("the instance of this run no longer exists"))?;

        // One run at a time per Odoo instance (dumps are heavy), and a global cap.
        let lock = self.instance_lock(instance_id);
        let _instance_guard = match lock.clone().try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => {
                self.log(run_id, "info", "Waiting for another run on this instance to finish…").await;
                tokio::select! {
                    guard = lock.lock_owned() => guard,
                    _ = cancel.cancelled() => return Err(Cancelled.into()),
                }
            }
        };
        if self.semaphore.available_permits() == 0 {
            self.log(run_id, "info", "Waiting for a free worker slot…").await;
        }
        let _permit = tokio::select! {
            permit = self.semaphore.clone().acquire_owned() => permit?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };

        db::query("UPDATE runs SET status = ?, started_at = ? WHERE id = ?")
            .bind(status::RUNNING)
            .bind(now())
            .bind(run_id)
            .execute(&self.db)
            .await?;
        self.publish_run(run_id).await;

        match run.kind.as_str() {
            "backup" => backup::run(&self, &run, cancel).await,
            "restore" => restore::run(&self, &run, cancel).await,
            other => bail!("unknown run kind '{other}'"),
        }
    }

    fn instance_lock(&self, instance_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        self.instance_locks.lock().unwrap().entry(instance_id).or_default().clone()
    }

    pub(crate) async fn log(&self, run_id: i64, level: &str, message: impl Into<String>) {
        let message = message.into();
        match level {
            "error" => tracing::error!(run_id, "{message}"),
            "warn" => tracing::warn!(run_id, "{message}"),
            _ => tracing::info!(run_id, "{message}"),
        }
        let ts = now();
        match db::query_scalar::<i64>(
            "INSERT INTO run_logs (run_id, ts, level, message) VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(run_id)
        .bind(ts)
        .bind(level)
        .bind(&message)
        .fetch_one(&self.db)
        .await
        {
            Ok(id) => {
                self.events.publish(Event::RunLog { log: RunLog { id, run_id, ts, level: level.to_string(), message } })
            }
            Err(err) => tracing::error!(run_id, error = %err, "cannot store run log"),
        }
    }

    pub(crate) async fn set_phase(&self, run_id: i64, phase: &str) {
        let _ = db::query("UPDATE runs SET phase = ? WHERE id = ?").bind(phase).bind(run_id).execute(&self.db).await;
        if let Some(active) = self.active.lock().unwrap().get_mut(&run_id) {
            active.progress = None;
        }
        self.publish_run(run_id).await;
    }

    pub(crate) async fn publish_run(&self, run_id: i64) {
        if let Ok(run) = get_run(&self.db, run_id).await {
            self.events.publish(Event::RunUpdated { run: Box::new(run) });
        }
    }

    /// Progress callback for a phase, throttled to ~2 events per second.
    pub(crate) fn tracker(
        self: &Arc<Self>,
        run_id: i64,
        phase: &str,
        label: Option<String>,
        total: Option<u64>,
    ) -> ProgressFn {
        let inner = Arc::clone(self);
        let phase = phase.to_string();
        let started = Instant::now();
        let last_emit = Mutex::new(Instant::now() - Duration::from_secs(10));
        Arc::new(move |done: u64| {
            let finished = total == Some(done);
            {
                let mut last = last_emit.lock().unwrap();
                if !finished && last.elapsed() < Duration::from_millis(500) {
                    return;
                }
                *last = Instant::now();
            }
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let progress = Progress {
                run_id,
                phase: phase.clone(),
                label: label.clone(),
                bytes_done: done,
                bytes_total: total,
                rate_bps: (done as f64 / elapsed) as u64,
            };
            if let Some(active) = inner.active.lock().unwrap().get_mut(&run_id) {
                active.progress = Some(progress.clone());
            }
            inner.events.publish(Event::RunProgress { progress });
        })
    }
}

/// Deletes a file when dropped.
pub struct TempFile(pub std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
