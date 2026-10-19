use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::auth::LoginLimiter;
use crate::config::Config;
use crate::crypto::Crypto;
use crate::db::Db;
use crate::engine::Engine;
use crate::events::EventBus;
use crate::storage::StorageContext;

/// A Google OAuth consent flow waiting for its callback.
pub struct PendingOAuth {
    pub destination_id: i64,
    pub redirect_uri: String,
    pub created: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<Config>,
    pub crypto: Arc<Crypto>,
    pub events: EventBus,
    pub engine: Engine,
    pub storage: StorageContext,
    pub limiter: Arc<LoginLimiter>,
    pub oauth_states: Arc<Mutex<HashMap<String, PendingOAuth>>>,
    pub scheduler_wake: Arc<Notify>,
    pub shutdown: CancellationToken,
    pub started_at: DateTime<Utc>,
}
