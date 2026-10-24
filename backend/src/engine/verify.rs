//! Integrity checks of downloaded backups before they are shipped anywhere.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::odoo::BackupFormat;
use crate::util::{Cancelled, ProgressFn, format_bytes};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VerifyReport {
    pub entries: usize,
    pub filestore_files: usize,
    pub dump_bytes: u64,
    pub odoo_version: Option<String>,
    pub db_name: Option<String>,
    pub pg_version: Option<String>,
    pub modules: Option<usize>,
    pub crc_checked: bool,
}

impl VerifyReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.entries == 0 {
            parts.push(format!("PostgreSQL custom-format dump, {}", format_bytes(self.dump_bytes)));
        } else {
            parts.push(format!("{} entries", self.entries));
            parts.push(format!("dump.sql {}", format_bytes(self.dump_bytes)));
            parts.push(format!("{} filestore files", self.filestore_files));
        }
        if let Some(v) = &self.odoo_version {
            parts.push(format!("Odoo {v}"));
        }
        if let Some(v) = &self.pg_version {
            parts.push(format!("PostgreSQL {v}"));
        }
        if let Some(m) = self.modules {
            parts.push(format!("{m} modules"));
        }
        if self.crc_checked {
            parts.push("all CRCs valid".into());
        }
        format!("Archive verified: {}", parts.join(", "))
    }
}

/// Sanity checks of a new backup against the previous successful one of the same database.
/// `drop_percent` is the shrink that raises a warning (0 disables the checks).
pub fn compare(
    previous_size: u64,
    previous: Option<&VerifyReport>,
    size: u64,
    current: &VerifyReport,
    drop_percent: u32,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if drop_percent == 0 {
        return warnings;
    }
    let shrink = |before: u64, after: u64| -> Option<u64> {
        (before > 0 && after < before).then(|| (before - after) * 100 / before).filter(|p| *p >= drop_percent as u64)
    };
    if let Some(pct) = shrink(previous_size, size) {
        warnings.push(format!(
            "the backup is {pct}% smaller than the previous one ({} → {})",
            format_bytes(previous_size),
            format_bytes(size)
        ));
    }
    // Small filestores fluctuate too much in relative terms to be meaningful.
    if let Some(prev) = previous.filter(|p| p.filestore_files >= 20 && current.entries > 0)
        && let Some(pct) = shrink(prev.filestore_files as u64, current.filestore_files as u64)
    {
        warnings.push(format!(
            "the filestore has {pct}% fewer files than in the previous backup ({} → {})",
            prev.filestore_files, current.filestore_files
        ));
    }
    if let (Some(before), Some(after)) = (previous.and_then(|p| p.modules), current.modules)
        && after < before
    {
        warnings.push(format!("{} fewer installed modules than in the previous backup", before - after));
    }
    warnings
}

/// Blocking: run inside `spawn_blocking`.
pub fn verify(
    path: &Path,
    format: BackupFormat,
    deep: bool,
    progress: &ProgressFn,
    cancel: &CancellationToken,
) -> anyhow::Result<VerifyReport> {
    match format {
        BackupFormat::Dump => verify_dump(path),
        BackupFormat::Zip => verify_zip(path, deep, progress, cancel),
    }
}

fn verify_dump(path: &Path) -> anyhow::Result<VerifyReport> {
    let mut header = [0u8; 5];
    File::open(path)?.read_exact(&mut header).context("the dump is truncated")?;
    if &header != b"PGDMP" {
        bail!("not a PostgreSQL custom-format dump");
    }
    Ok(VerifyReport { dump_bytes: std::fs::metadata(path)?.len(), ..Default::default() })
}

fn verify_zip(
    path: &Path,
    deep: bool,
    progress: &ProgressFn,
    cancel: &CancellationToken,
) -> anyhow::Result<VerifyReport> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(BufReader::with_capacity(1 << 20, file))
        .context("the backup is not a readable ZIP archive (truncated or corrupted)")?;
    let mut report = VerifyReport { entries: archive.len(), ..Default::default() };

    if let Ok(mut manifest) = archive.by_name("manifest.json") {
        let mut text = String::new();
        manifest.read_to_string(&mut text).context("manifest.json is corrupted")?;
        let value: Value = serde_json::from_str(&text).unwrap_or_default();
        report.odoo_version = value["version"].as_str().map(str::to_string);
        report.db_name = value["db_name"].as_str().map(str::to_string);
        report.pg_version = value["pg_version"].as_str().map(str::to_string);
        report.modules = value["modules"].as_object().map(|m| m.len());
    }

    {
        let dump = archive
            .by_name("dump.sql")
            .map_err(|_| anyhow!("dump.sql is missing from the archive: this is not an Odoo backup"))?;
        report.dump_bytes = dump.size();
    }
    if report.dump_bytes == 0 {
        bail!("dump.sql is empty");
    }

    if deep {
        let mut done = 0u64;
        let mut sink = std::io::sink();
        for index in 0..archive.len() {
            if cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            let mut entry = archive.by_index(index)?;
            let name = entry.name()?.to_string();
            if name.starts_with("filestore/") && !entry.is_dir() {
                report.filestore_files += 1;
            }
            let compressed = entry.compressed_size();
            // Reading an entry to the end validates its CRC-32.
            std::io::copy(&mut entry, &mut sink).with_context(|| format!("corrupted entry \"{name}\""))?;
            done += compressed;
            progress(done);
        }
        report.crc_checked = true;
    } else {
        report.filestore_files = archive
            .file_names()
            .filter_map(Result::ok)
            .filter(|n| n.starts_with("filestore/") && !n.ends_with('/'))
            .count();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::no_progress;
    use std::io::Write;

    fn make_zip(path: &Path, with_dump: bool) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if with_dump {
            zip.start_file("dump.sql", options).unwrap();
            zip.write_all(b"-- PostgreSQL database dump\nCREATE TABLE x();\n").unwrap();
        }
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(
            br#"{"version": "17.0", "db_name": "acme", "pg_version": "16.4", "modules": {"base": "17.0.1.3"}}"#,
        )
        .unwrap();
        zip.start_file("filestore/ab/abcdef", options).unwrap();
        zip.write_all(&[7u8; 4096]).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn valid_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.zip");
        make_zip(&path, true);
        let report = verify(&path, BackupFormat::Zip, true, &no_progress(), &CancellationToken::new()).unwrap();
        assert_eq!(report.odoo_version.as_deref(), Some("17.0"));
        assert_eq!(report.filestore_files, 1);
        assert_eq!(report.modules, Some(1));
        assert!(report.crc_checked);
    }

    #[test]
    fn missing_dump_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.zip");
        make_zip(&path, false);
        let err = verify(&path, BackupFormat::Zip, false, &no_progress(), &CancellationToken::new()).unwrap_err();
        assert!(err.to_string().contains("dump.sql"));
    }

    #[test]
    fn truncated_archive_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.zip");
        make_zip(&path, true);
        let data = std::fs::read(&path).unwrap();
        std::fs::write(&path, &data[..data.len() / 2]).unwrap();
        assert!(verify(&path, BackupFormat::Zip, true, &no_progress(), &CancellationToken::new()).is_err());
    }

    #[test]
    fn shrinking_backups_are_flagged() {
        let report = |files: usize, modules: usize| VerifyReport {
            entries: files + 2,
            filestore_files: files,
            modules: Some(modules),
            ..Default::default()
        };
        // Similar backup: nothing to report.
        assert!(compare(1000, Some(&report(100, 50)), 950, &report(98, 50), 30).is_empty());
        // Half the size and half the files.
        let warnings = compare(1000, Some(&report(100, 50)), 500, &report(50, 48), 30);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].starts_with("the backup is 50% smaller"));
        assert!(warnings[1].starts_with("the filestore has 50% fewer files"));
        assert_eq!(warnings[2], "2 fewer installed modules than in the previous backup");
        // Tiny filestores and older runs without a manifest only compare sizes.
        assert!(compare(1000, Some(&report(4, 50)), 990, &report(1, 50), 30).is_empty());
        assert_eq!(compare(1000, None, 100, &report(1, 50), 30).len(), 1);
        // Disabled.
        assert!(compare(1000, None, 1, &report(0, 0), 0).is_empty());
    }

    #[test]
    fn dump_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.dump");
        std::fs::write(&path, b"PGDMP\x01\x0e\x00").unwrap();
        assert!(verify(&path, BackupFormat::Dump, true, &no_progress(), &CancellationToken::new()).is_ok());
        std::fs::write(&path, b"<html>").unwrap();
        assert!(verify(&path, BackupFormat::Dump, true, &no_progress(), &CancellationToken::new()).is_err());
    }
}
