//! Run notifications: webhooks (generic JSON, Slack-compatible, Discord), Telegram and e-mail.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::crypto::Crypto;
use crate::db::{Artifact, NotificationChannelRow, Run, status};
use crate::util::{error_chain, format_bytes, format_duration};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    /// Full JSON payload (also has a `text` field).
    Generic,
    /// `{"text": ...}`: Slack, Mattermost, Rocket.Chat, Google Chat.
    Slack,
    /// `{"content": ...}`
    Discord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub format: WebhookFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub chat_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    Starttls,
    Tls,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    #[serde(default)]
    pub username: Option<String>,
    pub from: String,
    pub to: Vec<String>,
}

/// Secret part of a channel: webhook URL, Telegram bot token or SMTP password.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

pub struct Message {
    pub subject: String,
    pub text: String,
    pub event: String,
    pub data: Value,
}

#[derive(Clone)]
pub struct Notifier {
    db: SqlitePool,
    crypto: Arc<Crypto>,
    http: reqwest::Client,
    public_url: Option<String>,
}

impl Notifier {
    pub fn new(db: SqlitePool, crypto: Arc<Crypto>, public_url: Option<String>) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("odoo-backup/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { db, crypto, http, public_url })
    }

    /// Sends the outcome of a finished run to every matching channel.
    /// Returns `(channel name, result)` for each channel used.
    pub async fn run_finished(&self, run: &Run, artifacts: &[Artifact]) -> Vec<(String, anyhow::Result<()>)> {
        if run.status == status::CANCELLED {
            return Vec::new();
        }
        let success = run.status == status::SUCCESS;
        let channels: Vec<NotificationChannelRow> = match sqlx::query_as(
            "SELECT * FROM notification_channels WHERE enabled = 1 AND \
             ((? AND on_success = 1) OR (? AND on_failure = 1)) ORDER BY id",
        )
        .bind(success)
        .bind(!success)
        .fetch_all(&self.db)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                tracing::error!(error = %err, "cannot load notification channels");
                return Vec::new();
            }
        };
        if channels.is_empty() {
            return Vec::new();
        }
        let message = self.run_message(run, artifacts);
        let mut results = Vec::with_capacity(channels.len());
        for channel in channels {
            let result = self.send(&channel, &message).await;
            if let Err(err) = &result {
                tracing::warn!(channel = %channel.name, error = %format!("{err:#}"), "notification failed");
            }
            results.push((channel.name.clone(), result));
        }
        results
    }

    pub fn test_message(&self) -> Message {
        Message {
            subject: "Odoo Backup: test notification".into(),
            text: "✅ Test notification from Odoo Backup. If you can read this, the channel works.".into(),
            event: "test".into(),
            data: json!({}),
        }
    }

    fn run_message(&self, run: &Run, artifacts: &[Artifact]) -> Message {
        let restore = run.kind == "restore";
        let (icon, verdict) = match run.status.as_str() {
            status::SUCCESS => ("✅", "succeeded"),
            status::PARTIAL => ("⚠️", "partially failed"),
            _ => ("❌", "FAILED"),
        };
        let what = if restore { "Restore" } else { "Backup" };
        let subject = format!("{icon} {what} {verdict}: {} / {}", run.instance_name, run.database);

        let mut lines = vec![subject.clone()];
        let mut facts = Vec::new();
        if let Some(size) = run.size_bytes {
            facts.push(format!("Size: {}", format_bytes(size as u64)));
        }
        if let (Some(start), Some(end)) = (run.started_at, run.finished_at) {
            let secs = (end - start).num_milliseconds().max(0) as u64;
            facts.push(format!("Duration: {}", format_duration(Duration::from_millis(secs))));
        }
        if let Some(version) = &run.odoo_version {
            facts.push(format!("Odoo {version}"));
        }
        if !facts.is_empty() {
            lines.push(facts.join(" · "));
        }
        if !artifacts.is_empty() {
            let stored: Vec<String> = artifacts
                .iter()
                .map(|a| {
                    let mark = if a.status == status::ARTIFACT_STORED { "✓" } else { "✗" };
                    format!("{} {mark}", a.destination_name)
                })
                .collect();
            lines.push(format!("Destinations: {}", stored.join(", ")));
        }
        if let Some(job) = &run.job_name {
            lines.push(format!("Job: {job} ({})", run.trigger));
        }
        if let Some(error) = &run.error {
            lines.push(format!("Error: {error}"));
        }
        let link = self.public_url.as_ref().map(|base| format!("{base}/runs/{}", run.id));
        if let Some(link) = &link {
            lines.push(link.clone());
        }

        let event = format!("{}.{}", run.kind, run.status);
        Message {
            subject,
            text: lines.join("\n"),
            event,
            data: json!({
                "run": run,
                "artifacts": artifacts,
                "url": link,
            }),
        }
    }

    pub async fn send(&self, channel: &NotificationChannelRow, message: &Message) -> anyhow::Result<()> {
        let secret: ChannelSecret = match &channel.secret_enc {
            Some(enc) => serde_json::from_str(&self.crypto.decrypt(enc)?)?,
            None => ChannelSecret::default(),
        };
        match channel.kind.as_str() {
            "webhook" => {
                let config: WebhookConfig = serde_json::from_str(&channel.config_json)?;
                let url = secret.url.ok_or_else(|| anyhow!("webhook URL missing"))?;
                let payload = match config.format {
                    WebhookFormat::Generic => json!({
                        "event": message.event,
                        "subject": message.subject,
                        "text": message.text,
                        "data": message.data,
                    }),
                    WebhookFormat::Slack => json!({ "text": message.text }),
                    WebhookFormat::Discord => json!({ "content": truncate(&message.text, 1900) }),
                };
                self.post_json(&url, &payload).await
            }
            "telegram" => {
                let config: TelegramConfig = serde_json::from_str(&channel.config_json)?;
                let token = secret.bot_token.ok_or_else(|| anyhow!("bot token missing"))?;
                let url = format!("https://api.telegram.org/bot{token}/sendMessage");
                let payload = json!({
                    "chat_id": config.chat_id,
                    "text": truncate(&message.text, 4000),
                    "disable_web_page_preview": true,
                });
                self.post_json(&url, &payload).await.map_err(|e| {
                    // Never leak the bot token through error messages.
                    anyhow!("{}", format!("{e:#}").replace(&token, "***"))
                })
            }
            "email" => {
                let config: EmailConfig = serde_json::from_str(&channel.config_json)?;
                send_email(&config, secret.password.as_deref(), message).await
            }
            other => bail!("unknown channel type '{other}'"),
        }
    }

    async fn post_json(&self, url: &str, payload: &Value) -> anyhow::Result<()> {
        let resp = self
            .http
            .post(url)
            .json(payload)
            .send()
            .await
            .map_err(|e| anyhow!("request failed: {}", error_chain(&e.without_url())))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("HTTP {status}: {}", truncate(&body, 300));
        }
        Ok(())
    }
}

async fn send_email(config: &EmailConfig, password: Option<&str>, message: &Message) -> anyhow::Result<()> {
    let host = config.host.trim();
    let mut builder = match config.security {
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
        SmtpSecurity::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
        SmtpSecurity::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
    }
    .port(config.port)
    .timeout(Some(Duration::from_secs(30)));
    if let Some(user) = config.username.as_deref().filter(|u| !u.is_empty()) {
        builder = builder.credentials(Credentials::new(user.to_string(), password.unwrap_or_default().to_string()));
    }
    let mailer = builder.build();

    let mut email = lettre::Message::builder()
        .from(config.from.parse().with_context(|| format!("invalid sender address '{}'", config.from))?)
        .subject(&message.subject);
    for to in &config.to {
        email = email.to(to.parse().with_context(|| format!("invalid recipient address '{to}'"))?);
    }
    let email = email.header(ContentType::TEXT_PLAIN).body(message.text.clone())?;
    mailer.send(email).await.context("SMTP delivery failed")?;
    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}
