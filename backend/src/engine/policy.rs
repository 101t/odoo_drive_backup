//! 3-2-1-1-0 score of a job: 3 copies of the data, on 2 media, 1 off-site, 1 immutable,
//! 0 errors in the latest verified backups.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::db::status;
use crate::storage::Capabilities;

/// Latest finished backup of one database of the job.
pub struct LatestRun<'a> {
    pub status: &'a str,
    pub has_warning: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PolicyRule {
    /// `three_copies`, `two_media`, `one_off_site`, `one_immutable` or `zero_errors`.
    pub id: &'static str,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PolicyScore {
    pub score: usize,
    pub max: usize,
    pub rules: Vec<PolicyRule>,
}

pub fn score(destinations: &[Capabilities], verify_archive: bool, latest: &[LatestRun]) -> PolicyScore {
    let media: BTreeSet<_> = destinations.iter().map(|c| c.medium).collect();
    let rules = vec![
        // The live database is the first copy.
        PolicyRule { id: "three_copies", ok: destinations.len() >= 2 },
        PolicyRule { id: "two_media", ok: media.len() >= 2 },
        PolicyRule { id: "one_off_site", ok: destinations.iter().any(|c| c.off_site) },
        PolicyRule { id: "one_immutable", ok: destinations.iter().any(|c| c.immutable) },
        PolicyRule {
            id: "zero_errors",
            ok: verify_archive
                && !latest.is_empty()
                && latest.iter().all(|r| r.status == status::SUCCESS && !r.has_warning),
        },
    ];
    PolicyScore { score: rules.iter().filter(|r| r.ok).count(), max: rules.len(), rules }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::capabilities;

    fn failed(score: &PolicyScore) -> Vec<&'static str> {
        score.rules.iter().filter(|r| !r.ok).map(|r| r.id).collect()
    }

    #[test]
    fn single_local_destination_scores_low() {
        let local = capabilities("local", r#"{"path": "/backups"}"#);
        let ok = [LatestRun { status: "success", has_warning: false }];
        let result = score(&[local], true, &ok);
        assert_eq!(result.score, 1);
        assert_eq!(failed(&result), ["three_copies", "two_media", "one_off_site", "one_immutable"]);
    }

    #[test]
    fn local_plus_cloud() {
        let local = capabilities("local", r#"{"path": "/backups"}"#);
        let drive = capabilities("gdrive", "{}");
        let ok = [LatestRun { status: "success", has_warning: false }];
        let result = score(&[local, drive], true, &ok);
        assert_eq!((result.score, result.max), (4, 5));
        assert_eq!(failed(&result), ["one_immutable"]);

        // A mounted remote directory counts as off-site.
        let remote = capabilities("local", r#"{"path": "/mnt/nas", "off_site": true}"#);
        assert!(score(&[local, remote], true, &ok).rules[2].ok);
    }

    #[test]
    fn errors_and_warnings_fail_the_last_rule() {
        let drive = capabilities("gdrive", "{}");
        let warned = [LatestRun { status: "success", has_warning: true }];
        let partial = [LatestRun { status: "partial", has_warning: false }];
        let ok = [LatestRun { status: "success", has_warning: false }];
        assert!(!score(&[drive], true, &warned).rules[4].ok);
        assert!(!score(&[drive], true, &partial).rules[4].ok);
        assert!(!score(&[drive], true, &[]).rules[4].ok, "never ran");
        assert!(!score(&[drive], false, &ok).rules[4].ok, "deep verification disabled");
        assert!(score(&[drive], true, &ok).rules[4].ok);
    }
}
