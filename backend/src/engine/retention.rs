//! Grandfather-father-son retention ("keep last N, N daily, N weekly, N monthly, N yearly").

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::EngineInner;
use crate::db::{self, JobRow, get_destination, status};
use crate::storage::Storage;
use crate::util::now;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
    pub keep_yearly: u32,
}

impl RetentionPolicy {
    /// All counters at zero means "never delete anything".
    pub fn is_unlimited(&self) -> bool {
        self.keep_last == 0
            && self.keep_daily == 0
            && self.keep_weekly == 0
            && self.keep_monthly == 0
            && self.keep_yearly == 0
    }
}

/// Returns the IDs of backups to delete. `items` are `(id, creation time)` in any order.
/// The newest backup is always kept.
pub fn select_to_delete<Tz: TimeZone>(items: &[(i64, DateTime<Utc>)], policy: &RetentionPolicy, tz: &Tz) -> Vec<i64> {
    if policy.is_unlimited() || items.is_empty() {
        return Vec::new();
    }
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

    let mut keep: HashSet<i64> = HashSet::new();
    keep.insert(sorted[0].0);
    for (id, _) in sorted.iter().take(policy.keep_last as usize) {
        keep.insert(*id);
    }

    let mut keep_buckets = |count: u32, key: &dyn Fn(&DateTime<Tz>) -> (i32, u32)| {
        if count == 0 {
            return;
        }
        let mut last_key = None;
        let mut used = 0;
        for (id, ts) in &sorted {
            let k = key(&ts.with_timezone(tz));
            if last_key != Some(k) {
                last_key = Some(k);
                if used == count {
                    break;
                }
                keep.insert(*id);
                used += 1;
            }
        }
    };
    keep_buckets(policy.keep_daily, &|t| (t.year(), t.ordinal()));
    keep_buckets(policy.keep_weekly, &|t| {
        let w = t.iso_week();
        (w.year(), w.week())
    });
    keep_buckets(policy.keep_monthly, &|t| (t.year(), t.month()));
    keep_buckets(policy.keep_yearly, &|t| (t.year(), 0));

    sorted.iter().filter(|(id, _)| !keep.contains(id)).map(|(id, _)| *id).collect()
}

/// Applies the job's retention policy to every destination holding backups of `database`.
/// Pinned backups are outside the policy: never deleted, and they do not use up its slots.
pub(super) async fn apply(engine: &Arc<EngineInner>, job: &JobRow, database: &str, run_id: i64) {
    let policy = job.retention();
    if policy.is_unlimited() {
        engine.log(run_id, "info", "Retention: disabled, all backups are kept").await;
        return;
    }
    let tz: chrono_tz::Tz = job.timezone.parse().unwrap_or(chrono_tz::UTC);

    let rows: Vec<(i64, i64, String, DateTime<Utc>)> = match db::query_as(
        "SELECT a.id, a.destination_id, a.remote_id, r.created_at FROM artifacts a \
         JOIN runs r ON r.id = a.run_id \
         WHERE r.job_id = ? AND r.database = ? AND r.kind = 'backup' AND a.status = ? \
           AND a.destination_id IS NOT NULL AND a.remote_id IS NOT NULL AND a.pinned = FALSE",
    )
    .bind(job.id)
    .bind(database)
    .bind(status::ARTIFACT_STORED)
    .fetch_all(&engine.db)
    .await
    {
        Ok(rows) => rows,
        Err(err) => {
            engine.log(run_id, "warn", format!("Retention skipped: {err}")).await;
            return;
        }
    };

    let mut by_destination: BTreeMap<i64, Vec<(i64, String, DateTime<Utc>)>> = BTreeMap::new();
    for (artifact_id, destination_id, remote_id, created) in rows {
        by_destination.entry(destination_id).or_default().push((artifact_id, remote_id, created));
    }

    for (destination_id, items) in by_destination {
        let pairs: Vec<(i64, DateTime<Utc>)> = items.iter().map(|(id, _, ts)| (*id, *ts)).collect();
        let doomed: HashSet<i64> = select_to_delete(&pairs, &policy, &tz).into_iter().collect();
        if doomed.is_empty() {
            continue;
        }
        let destination = match get_destination(&engine.db, destination_id).await {
            Ok(d) => d,
            Err(err) => {
                engine.log(run_id, "warn", format!("Retention skipped for destination #{destination_id}: {err}")).await;
                continue;
            }
        };
        let storage = match Storage::open(&destination, &engine.crypto, &engine.storage) {
            Ok(s) => s,
            Err(err) => {
                engine.log(run_id, "warn", format!("Retention skipped on {}: {err:#}", destination.name)).await;
                continue;
            }
        };
        let mut removed = 0;
        for (artifact_id, remote_id, _) in items.iter().filter(|(id, _, _)| doomed.contains(id)) {
            match storage.delete(remote_id).await {
                Ok(()) => {
                    let _ = db::query(
                        "UPDATE artifacts SET status = ?, deleted_at = ?, delete_reason = 'retention' WHERE id = ?",
                    )
                    .bind(status::ARTIFACT_DELETED)
                    .bind(now())
                    .bind(artifact_id)
                    .execute(&engine.db)
                    .await;
                    removed += 1;
                }
                Err(err) => {
                    engine
                        .log(
                            run_id,
                            "warn",
                            format!("Retention: cannot delete {remote_id} on {}: {err:#}", destination.name),
                        )
                        .await;
                }
            }
        }
        engine
            .log(
                run_id,
                "info",
                format!(
                    "Retention on {}: kept {}, removed {removed} old backup(s)",
                    destination.name,
                    items.len() - removed
                ),
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn daily(n: i64) -> Vec<(i64, DateTime<Utc>)> {
        let newest = Utc.with_ymd_and_hms(2026, 10, 19, 2, 0, 0).unwrap();
        (0..n).map(|i| (n - i, newest - Duration::days(i))).collect()
    }

    #[test]
    fn unlimited_keeps_everything() {
        assert!(select_to_delete(&daily(30), &RetentionPolicy::default(), &Utc).is_empty());
    }

    #[test]
    fn keep_last_only() {
        let policy = RetentionPolicy { keep_last: 3, ..Default::default() };
        let mut deleted = select_to_delete(&daily(10), &policy, &Utc);
        deleted.sort();
        assert_eq!(deleted, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn newest_is_always_kept() {
        let policy = RetentionPolicy { keep_monthly: 1, ..Default::default() };
        let items = daily(5);
        let deleted = select_to_delete(&items, &policy, &Utc);
        assert!(!deleted.contains(&5));
        assert_eq!(deleted.len(), 4);
    }

    #[test]
    fn daily_buckets_keep_newest_of_each_day() {
        let base = Utc.with_ymd_and_hms(2026, 10, 19, 0, 0, 0).unwrap();
        // Three backups per day over three days, ids increasing with time.
        let mut items = Vec::new();
        let mut id = 0;
        for day in (0..3).rev() {
            for hour in [1, 9, 17] {
                id += 1;
                items.push((id, base - Duration::days(day) + Duration::hours(hour)));
            }
        }
        let policy = RetentionPolicy { keep_daily: 2, ..Default::default() };
        let mut deleted = select_to_delete(&items, &policy, &Utc);
        deleted.sort();
        // Kept: 9 (newest of today) and 6 (newest of yesterday).
        assert_eq!(deleted, vec![1, 2, 3, 4, 5, 7, 8]);
    }

    #[test]
    fn gfs_policy_bounds() {
        let policy = RetentionPolicy { keep_last: 3, keep_daily: 7, keep_weekly: 4, keep_monthly: 6, keep_yearly: 1 };
        let items = daily(400);
        let deleted = select_to_delete(&items, &policy, &Utc);
        let kept: Vec<i64> = items.iter().map(|(id, _)| *id).filter(|id| !deleted.contains(id)).collect();
        // The last 7 days are always present.
        for id in 394..=400 {
            assert!(kept.contains(&id), "backup {id} should be kept");
        }
        // At most last + daily + weekly + monthly + yearly backups survive.
        assert!(kept.len() <= 3 + 7 + 4 + 6 + 1);
        // Something very old was pruned.
        assert!(deleted.contains(&100));
    }

    #[test]
    fn buckets_follow_timezone() {
        // 22:30 UTC on Oct 18 is already Oct 19 in Asia/Beirut (UTC+3).
        let tz: chrono_tz::Tz = "Asia/Beirut".parse().unwrap();
        let items = vec![
            (1, Utc.with_ymd_and_hms(2026, 10, 18, 22, 30, 0).unwrap()),
            (2, Utc.with_ymd_and_hms(2026, 10, 19, 5, 0, 0).unwrap()),
        ];
        let policy = RetentionPolicy { keep_daily: 1, ..Default::default() };
        assert_eq!(select_to_delete(&items, &policy, &tz), vec![1]);
        assert!(select_to_delete(&items, &RetentionPolicy { keep_daily: 2, ..Default::default() }, &Utc).is_empty());
    }
}
