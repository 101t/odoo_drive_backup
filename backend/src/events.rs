//! In-process event bus feeding the Server-Sent Events endpoint.

use serde::Serialize;
use tokio::sync::broadcast;

use crate::db::{Run, RunLog};

#[derive(Debug, Clone, Serialize, Default)]
pub struct Progress {
    pub run_id: i64,
    /// `downloading`, `verifying`, `uploading`, `fetching`, `restoring`.
    pub phase: String,
    /// Destination name while uploading.
    pub label: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub rate_bps: u64,
}

/// Serialized with a `type` field (`run_updated`, `run_progress`, `run_log`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum Event {
    RunUpdated { run: Box<Run> },
    RunProgress { progress: Progress },
    RunLog { log: RunLog },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(2048);
        Self { tx }
    }

    pub fn publish(&self, event: Event) {
        // No subscribers is fine.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
