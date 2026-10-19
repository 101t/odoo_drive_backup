use std::net::SocketAddr;
use std::path::PathBuf;

/// Runtime configuration. Every option can be set with a CLI flag or an environment variable.
#[derive(clap::Args, Clone, Debug)]
pub struct Config {
    /// Address the HTTP server listens on.
    #[arg(long, env = "ODB_BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    /// Directory holding the SQLite database, the encryption key and temporary files.
    #[arg(long, env = "ODB_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    /// Keep the application data in PostgreSQL instead of the SQLite file in the data directory,
    /// e.g. postgres://user:password@host:5432/odoo_backup (`?sslmode=require` for TLS).
    #[arg(long, env = "ODB_DATABASE_URL", hide_env_values = true)]
    pub database_url: Option<String>,

    /// Where backups are buffered while they travel from Odoo to the destinations.
    /// Needs enough free space for the largest backup. Defaults to `<data_dir>/tmp`.
    #[arg(long, env = "ODB_TEMP_DIR")]
    pub temp_dir: Option<PathBuf>,

    /// Base64-encoded 32-byte key used to encrypt stored secrets.
    /// When unset, a key is generated and kept in `<data_dir>/secret.key`.
    #[arg(long, env = "ODB_SECRET_KEY", hide_env_values = true)]
    pub secret_key: Option<String>,

    /// Maximum number of backup/restore runs executing at the same time.
    #[arg(long, env = "ODB_MAX_CONCURRENT_JOBS", default_value_t = 2)]
    pub max_concurrent_jobs: usize,

    /// Public URL of this service (e.g. https://backup.example.com). Used for the
    /// Google OAuth redirect URI and links in notifications.
    #[arg(long, env = "ODB_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Mark the session cookie as `Secure` (enable when served over HTTPS).
    #[arg(long, env = "ODB_COOKIE_SECURE", default_value_t = false, action = clap::ArgAction::Set)]
    pub cookie_secure: bool,

    /// Session lifetime in hours.
    #[arg(long, env = "ODB_SESSION_HOURS", default_value_t = 168)]
    pub session_hours: i64,

    /// Create this admin user on first start (only when no user exists yet).
    #[arg(long, env = "ODB_ADMIN_USERNAME")]
    pub admin_username: Option<String>,

    /// Password for `ODB_ADMIN_USERNAME`.
    #[arg(long, env = "ODB_ADMIN_PASSWORD", hide_env_values = true)]
    pub admin_password: Option<String>,

    /// Bearer token protecting the Prometheus `/metrics` endpoint. The endpoint is disabled when unset.
    #[arg(long, env = "ODB_METRICS_TOKEN", hide_env_values = true)]
    pub metrics_token: Option<String>,

    /// Chunk size for Google Drive resumable uploads, in MiB.
    #[arg(long, env = "ODB_UPLOAD_CHUNK_MB", default_value_t = 16)]
    pub upload_chunk_mb: usize,

    #[arg(long, env = "ODB_GOOGLE_API_BASE", default_value = "https://www.googleapis.com", hide = true)]
    pub google_api_base: String,

    #[arg(long, env = "ODB_GOOGLE_TOKEN_URL", default_value = "https://oauth2.googleapis.com/token", hide = true)]
    pub google_token_url: String,

    #[arg(
        long,
        env = "ODB_GOOGLE_AUTH_URL",
        default_value = "https://accounts.google.com/o/oauth2/v2/auth",
        hide = true
    )]
    pub google_auth_url: String,
}

impl Config {
    pub fn temp_dir(&self) -> PathBuf {
        self.temp_dir.clone().unwrap_or_else(|| self.data_dir.join("tmp"))
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("odoo-backup.db")
    }

    /// Upload chunk size in bytes, rounded to the 256 KiB multiple Google Drive requires.
    pub fn upload_chunk_bytes(&self) -> usize {
        const QUANTUM: usize = 256 * 1024;
        let wanted = self.upload_chunk_mb.clamp(1, 512) * 1024 * 1024;
        (wanted / QUANTUM).max(1) * QUANTUM
    }

    pub fn public_url(&self) -> Option<String> {
        self.public_url.as_deref().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty())
    }
}
