/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use derived_data::BonsaiDerived;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataRef;
use slog::info;

use crate::commit_sync_outcome::CommitSyncOutcome;

/// Name of the commit extra. This extra forces a commit to
/// be rewritten with a particular commit sync config version.
pub const CHANGE_XREPO_MAPPING_EXTRA: &str = "change-xrepo-mapping-to-version";

/// For merge commit `source_cs_id` and `parent_outcomes` for
/// its parents, get the version to use to construct a mover
pub async fn get_version_for_merge<'a>(
    ctx: &CoreContext,
    repo: &(impl RepoDerivedDataRef + Send + Sync),
    source_cs_id: ChangesetId,
    parent_outcomes: impl IntoIterator<Item = &'a CommitSyncOutcome>,
) -> Result<CommitSyncConfigVersion, Error> {
    let cs_info = ChangesetInfo::derive(ctx, repo, source_cs_id).await?;
    get_version_for_merge_with_info(ctx, &cs_info, parent_outcomes)
}

pub fn get_version_for_merge_with_info<'a>(
    ctx: &CoreContext,
    cs_info: &ChangesetInfo,
    parent_outcomes: impl IntoIterator<Item = &'a CommitSyncOutcome>,
) -> Result<CommitSyncConfigVersion, Error> {
    if let Some(version) = get_mapping_change_version(cs_info)? {
        info!(
            ctx.logger(),
            "force using mapping {} to rewrite {}",
            version,
            cs_info.changeset_id()
        );
        return Ok(version);
    }

    get_version_for_merge_impl(cs_info.changeset_id(), parent_outcomes)
}

fn get_version_for_merge_impl<'a>(
    source_cs_id: &ChangesetId,
    parent_outcomes: impl IntoIterator<Item = &'a CommitSyncOutcome>,
) -> Result<CommitSyncConfigVersion, Error> {
    use CommitSyncOutcome::*;
    let maybe_version = get_version_impl(
        *source_cs_id,
        parent_outcomes
            .into_iter()
            .filter_map(|parent_outcome| match parent_outcome {
                NotSyncCandidate(_) => None,
                RewrittenAs(_, version) | EquivalentWorkingCopyAncestor(_, version) => {
                    Some(version)
                }
            }),
    )?;

    maybe_version.ok_or_else(|| {
        format_err!(
            "unexpected absence of rewritten parents for {}",
            source_cs_id,
        )
    })
}

pub async fn get_version<'a>(
    ctx: &CoreContext,
    repo: &(impl RepoDerivedDataRef + Send + Sync),
    source_cs_id: ChangesetId,
    parent_versions: impl IntoIterator<Item = &'a CommitSyncConfigVersion>,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    let cs_info = ChangesetInfo::derive(ctx, repo, source_cs_id).await?;
    get_version_with_info(ctx, &cs_info, parent_versions)
}

pub fn get_version_with_info<'a>(
    ctx: &CoreContext,
    cs_info: &ChangesetInfo,
    parent_versions: impl IntoIterator<Item = &'a CommitSyncConfigVersion>,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    if let Some(version) = get_mapping_change_version(cs_info)? {
        info!(
            ctx.logger(),
            "force using mapping {} to rewrite {}",
            version,
            cs_info.changeset_id()
        );
        return Ok(Some(version));
    }

    get_version_impl(*cs_info.changeset_id(), parent_versions)
}

fn get_version_impl<'a>(
    source_cs_id: ChangesetId,
    parent_versions: impl IntoIterator<Item = &'a CommitSyncConfigVersion>,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    let versions: HashSet<_> = parent_versions.into_iter().collect();
    let mut iter = versions.into_iter();
    match (iter.next(), iter.next()) {
        (Some(v1), Some(v2)) => Err(format_err!(
            "too many CommitSyncConfig versions used to remap parents of {}: {}, {} (may be more)",
            source_cs_id,
            v1,
            v2,
        )),
        (Some(v1), None) => Ok(Some(v1.clone())),
        (None, _) => Ok(None),
    }
}

/// Get a mapping change version from changeset extras, if present
/// Some changesets are used as "boundaries" to change CommmitSyncConfigVersion
/// used in syncing. This is determined by the `CHANGE_XREPO_MAPPING_EXTRA`'s
/// value.
pub fn get_mapping_change_version(
    cs_info: &ChangesetInfo,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    if tunables::tunables()
        .allow_change_xrepo_mapping_extra()
        .unwrap_or(false)
    {
        let maybe_mapping = cs_info
            .hg_extra()
            .find(|(name, _)| name == &CHANGE_XREPO_MAPPING_EXTRA);
        if let Some((_, version)) = maybe_mapping {
            let version = String::from_utf8(version.to_vec()).with_context(|| {
                format!("non-utf8 version is set in {}", cs_info.changeset_id())
            })?;

            let version = CommitSyncConfigVersion(version);
            return Ok(Some(version));
        }
    }
    Ok(None)
}

/// Set mapping change version into changeset extras
/// Some changesets are used as "boundaries" to change CommmitSyncConfigVersion
/// used in syncing. This is determined by the `CHANGE_XREPO_MAPPING_EXTRA`'s
/// value.
pub fn set_mapping_change_version(
    bcs: &mut BonsaiChangesetMut,
    mapping_version: CommitSyncConfigVersion,
) -> Result<(), Error> {
    if bcs
        .hg_extra
        .contains_key(&CHANGE_XREPO_MAPPING_EXTRA.to_string())
    {
        return Err(format_err!(
            "changeset already contains the {}",
            CHANGE_XREPO_MAPPING_EXTRA
        ));
    }
    bcs.hg_extra.insert(
        CHANGE_XREPO_MAPPING_EXTRA.to_string(),
        mapping_version.0.clone().into_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use mononoke_types_mocks::changesetid as bonsai;

    use super::*;

    #[fbinit::test]
    fn test_merge_version_determinator_success_single_rewritten(_fb: FacebookInit) {
        // Basic test: there's a single non-preserved parent, determining
        // Mover version should succeed
        use CommitSyncOutcome::*;
        let v1 = CommitSyncConfigVersion("TEST_VERSION_1".to_string());
        let parent_outcomes = [
            NotSyncCandidate(v1.clone()),
            RewrittenAs(bonsai::FOURS_CSID, v1.clone()),
        ];

        let rv = get_version_for_merge_impl(&bonsai::ONES_CSID, &parent_outcomes).unwrap();
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

        let rv = get_version_for_merge_impl(&bonsai::ONES_CSID, &parent_outcomes).unwrap();
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

        let e = get_version_for_merge_impl(&bonsai::ONES_CSID, &parent_outcomes).unwrap_err();
        assert!(
            format!("{}", e).contains("too many CommitSyncConfig versions used to remap parents")
        );
    }

    #[fbinit::test]
    fn test_merge_version_determinator_failure_all_not_candidates(_fb: FacebookInit) {
        // All parents are preserved, this function should not have been called
        use CommitSyncOutcome::*;
        let v1 = CommitSyncConfigVersion("TEST_VERSION_1".to_string());
        let parent_outcomes = [NotSyncCandidate(v1.clone()), NotSyncCandidate(v1)];

        let e = get_version_for_merge_impl(&bonsai::ONES_CSID, &parent_outcomes).unwrap_err();
        assert!(format!("{}", e).contains("unexpected absence of rewritten parents"));
    }
}
