use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

/// Callback receiving the number of bytes processed so far.
pub type ProgressFn = Arc<dyn Fn(u64) + Send + Sync>;

#[cfg(test)]
pub fn no_progress() -> ProgressFn {
    Arc::new(|_| {})
}

/// Marker error for operations aborted through a cancellation token.
#[derive(Debug, thiserror::Error)]
#[error("cancelled by user")]
pub struct Cancelled;

pub fn is_cancelled(err: &anyhow::Error) -> bool {
    err.chain().any(|e| e.is::<Cancelled>())
}

/// Formats an error with its whole `source()` chain (reqwest hides the useful part there).
pub fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..=59 => format!("{}.{}s", secs, d.subsec_millis() / 100),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

pub fn format_rate(bytes: u64, elapsed: Duration) -> String {
    let secs = elapsed.as_secs_f64().max(0.001);
    format!("{}/s", format_bytes((bytes as f64 / secs) as u64))
}

/// Turns arbitrary text into a safe single path segment / file-name component.
pub fn sanitize_segment(input: &str) -> String {
    let cleaned: String = input
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').trim().to_string();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

/// Lowercase ASCII slug used in backup file names.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "odoo".to_string() } else { out }
}

/// Free bytes available to unprivileged users on the filesystem holding `path`.
pub fn available_space(path: &Path) -> Option<u64> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    #[allow(clippy::unnecessary_cast)]
    Some(stat.blocks_available() as u64 * stat.fragment_size() as u64)
}

/// Exponential backoff with jitter: base * 2^attempt, capped.
pub fn backoff(attempt: u32, base: Duration, cap: Duration) -> Duration {
    let exp = base.saturating_mul(1u32 << attempt.min(16));
    let capped = exp.min(cap);
    let jitter = (getrandom::u32().unwrap_or(0) % 1000) as u64;
    capped + Duration::from_millis(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_human_readable() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn segments_are_sanitized() {
        assert_eq!(sanitize_segment("../etc/passwd"), "_etc_passwd");
        assert_eq!(sanitize_segment("  prod db  "), "prod db");
        assert_eq!(sanitize_segment(".."), "_");
        assert_eq!(sanitize_segment("a\\b:c"), "a_b_c");
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("ACME Production (EU)"), "acme-production-eu");
        assert_eq!(slugify("***"), "odoo");
    }
}
