/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Error};
use iterhelpers::get_only_item;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use std::collections::HashSet;

use crate::commit_sync_outcome::CommitSyncOutcome;

/// For merge commit `source_cs_is` and `parent_outcomes` for
/// its parents, get the version to use to construct a mover
pub fn get_version_for_merge<'a>(
    source_cs_id: ChangesetId,
    parent_outcomes: impl IntoIterator<Item = &'a CommitSyncOutcome>,
) -> Result<CommitSyncConfigVersion, Error> {
    let unique_versions = {
        let mut versions = HashSet::new();
        for parent_outcome in parent_outcomes.into_iter() {
            use CommitSyncOutcome::*;
            match parent_outcome {
                NotSyncCandidate => continue,
                Preserved => {
                    bail!("cannot syncs merges of rewritten and preserved commits");
                }
                RewrittenAs(_, version) | EquivalentWorkingCopyAncestor(_, version) => {
                    versions.insert(version.clone());
                }
            }
        }

        versions
    };

    let version_res: Result<_, Error> = get_only_item(
        unique_versions,
        || {
            format_err!(
                "unexpected absence of rewritten parents for {}",
                source_cs_id,
            )
        },
        |v1, v2| {
            format_err!(
                "too many CommitSyncConfig versions used to remap parents of {}: {}, {} (may be more)",
                source_cs_id,
                v1,
                v2,
            )
        },
    );
    // Type inference cannot figure the error type on its own
    let version = version_res?;

    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbinit::FacebookInit;
    use mononoke_types_mocks::changesetid as bonsai;

    #[fbinit::test]
    fn test_merge_version_determinator_success_single_rewritten(_fb: FacebookInit) {
        // Basic test: there's a single non-preserved parent, determining
        // Mover version should succeed
        use CommitSyncOutcome::*;
        let v1 = CommitSyncConfigVersion("TEST_VERSION_1".to_string());
        let parent_outcomes = [
            NotSyncCandidate,
            RewrittenAs(bonsai::FOURS_CSID, v1.clone()),
        ];

        let rv = get_version_for_merge(bonsai::ONES_CSID, &parent_outcomes).unwrap();
        assert_eq!(rv, v1);
    }

    #[fbinit::test]
    fn test_merge_version_determinator_success(_fb: FacebookInit) {
        // There are two rewritten parents, both preserved with the same
        // version. Determining Mover version should succeed
        use CommitSyncOutcome::*;
        let v1 = CommitSyncConfigVersion("TEST_VERSION_1".to_string());
        let parent_outcomes = [
            RewrittenAs(bonsai::FOURS_CSID, v1.clone()),
            RewrittenAs(bonsai::THREES_CSID, v1.clone()),
        ];

        let rv = get_version_for_merge(bonsai::ONES_CSID, &parent_outcomes).unwrap();
        assert_eq!(rv, v1);
    }

    #[fbinit::test]
    fn test_merge_version_determinator_failure_different_versions(_fb: FacebookInit) {
        // There are two rewritten parents, with different versions
        // Determining Mover version should fail
        use CommitSyncOutcome::*;
        let v1 = CommitSyncConfigVersion("TEST_VERSION_1".to_string());
        let v2 = CommitSyncConfigVersion("TEST_VERSION_2".to_string());
        let parent_outcomes = [
            RewrittenAs(bonsai::FOURS_CSID, v1),
            RewrittenAs(bonsai::THREES_CSID, v2),
        ];

        let e = get_version_for_merge(bonsai::ONES_CSID, &parent_outcomes).unwrap_err();
        assert!(
            format!("{}", e).contains("too many CommitSyncConfig versions used to remap parents")
        );
    }

    #[fbinit::test]
    fn test_merge_version_determinator_failure_all_not_candidates(_fb: FacebookInit) {
        // All parents are preserved, this function should not have been called
        use CommitSyncOutcome::*;
        let parent_outcomes = [NotSyncCandidate, NotSyncCandidate];

        let e = get_version_for_merge(bonsai::ONES_CSID, &parent_outcomes).unwrap_err();
        assert!(format!("{}", e).contains("unexpected absence of rewritten parents"));
    }
}
