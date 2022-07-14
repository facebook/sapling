/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::find_target_bookmark_and_value;
use crate::common::find_target_sync_config;
use crate::common::MegarepoOp;
use anyhow::anyhow;
use context::CoreContext;
use core::cmp::Ordering;
use derived_data_utils::derived_data_utils;
use futures::future;
use futures::stream::FuturesUnordered;
use futures::TryStreamExt;
use itertools::EitherOrBoth;
use itertools::Itertools;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Source;
use megarepo_config::SyncConfigVersion;
use megarepo_config::SyncTargetConfig;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::SourceName;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mutable_renames::MutableRenames;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Structure representing changes needed to be applied onto target to change its
/// config.
/// All the changes will be realized by removing and readding the source. In the
/// future we can make this datastructure richer and include less disruptive
/// methods of introducing small changes (like for example adding a linkfile).
struct SyncTargetConfigChanges {
    added: Vec<(Source, ChangesetId)>,
    removed: Vec<(Source, ChangesetId)>,
}

/// Comparator used for sorting the sources.
fn cmp_by_name(a: &Source, b: &Source) -> Ordering {
    Ord::cmp(&a.source_name, &b.source_name)
}

/// Compares the current state with the desired end state and returns the changes
/// needed to apply to current state.
fn diff_configs(
    old_config: &SyncTargetConfig,
    old_changesets: &BTreeMap<SourceName, ChangesetId>,
    new_config: &SyncTargetConfig,
    new_changesets: &BTreeMap<SourceName, ChangesetId>,
) -> Result<SyncTargetConfigChanges, MegarepoError> {
    let old_sources = old_config
        .sources
        .clone()
        .into_iter()
        .sorted_by(cmp_by_name);
    let new_sources = new_config
        .sources
        .clone()
        .into_iter()
        .sorted_by(cmp_by_name);
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for merged in old_sources.merge_join_by(new_sources, cmp_by_name) {
        match merged {
            EitherOrBoth::Left(old_source) => {
                let cs_id = old_changesets
                    .get(&SourceName::new(&old_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "remapping state is missing mapping for {}",
                            &old_source.source_name
                        ))
                    })?;
                removed.push((old_source, *cs_id));
            }
            EitherOrBoth::Right(new_source) => {
                let cs_id = new_changesets
                    .get(&SourceName::new(&new_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "changesets_to_merge is missing mapping for {}",
                            &new_source.source_name
                        ))
                    })?;
                added.push((new_source, *cs_id));
            }
            EitherOrBoth::Both(old_source, new_source) => {
                let old_cs_id = old_changesets
                    .get(&SourceName::new(&old_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "remapping state is missing mapping for {}",
                            &old_source.source_name
                        ))
                    })?;
                let new_cs_id = new_changesets
                    .get(&SourceName::new(&new_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "changesets_to_merge is missing mapping for {}",
                            &new_source.source_name
                        ))
                    })?;
                if old_source != new_source || old_cs_id != new_cs_id {
                    removed.push((old_source, *old_cs_id));
                    added.push((new_source, *new_cs_id));
                }
            }
        }
    }
    Ok(SyncTargetConfigChanges { added, removed })
}

/// Change target config given a new config. After this command finishes it
/// creates move commits on top of source commits (for the newly added and
/// changed sources), merges them all together then merges them with current head
/// of the target while removing all files belonging to removed sources.
///
/// ```text
///        Tn+1
///       /    \
///      X      Tn
///     / \      \
///    M   M      \
///   /     \      \
///  S       S      \
///
/// Tn - pre-change (old) head of the target branch
/// Tn+1 - post-change (new) head of the target branch (merges in X, and does removals)
/// X - target merge commits
/// M - move commits
/// S - source commits that need to be merged
/// ```
pub struct ChangeTargetConfig<'a> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke>,
    pub mutable_renames: &'a Arc<MutableRenames>,
}

impl<'a> MegarepoOp for ChangeTargetConfig<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        self.mononoke
    }
}

impl<'a> ChangeTargetConfig<'a> {
    pub fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke>,
        mutable_renames: &'a Arc<MutableRenames>,
    ) -> Self {
        Self {
            megarepo_configs,
            mononoke,
            mutable_renames,
        }
    }

    pub async fn run(
        self,
        ctx: &CoreContext,
        target: &Target,
        new_version: SyncConfigVersion,
        target_location: ChangesetId,
        changesets_to_merge: BTreeMap<SourceName, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let target_repo = self.find_repo_by_id(ctx, target.repo_id).await?;

        // Find the target config version and remapping state that was used to
        // create the latest target commit. This config version will be used to
        // as a base for comparing with new config.
        let (target_bookmark, actual_target_location) =
            find_target_bookmark_and_value(ctx, &target_repo, target).await?;

        // target doesn't point to the commit we expect - check
        // if this method has already succeded and just immediately return the
        // result if so.
        if actual_target_location != target_location {
            return self
                .check_if_this_method_has_already_succeeded(
                    ctx,
                    &new_version,
                    (target_location, actual_target_location),
                    &changesets_to_merge,
                    &target_repo,
                )
                .await;
        }

        let old_target_cs = &target_repo
            .changeset(target_location)
            .await?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!("programming error - target changeset not found!"))
            })?;
        let (old_remapping_state, old_config) = find_target_sync_config(
            ctx,
            target_repo.blob_repo(),
            target_location,
            target,
            self.megarepo_configs,
        )
        .await?;

        // Contruct the new config structure and the remapping state
        let new_config = self.megarepo_configs.get_config_by_version(
            ctx.clone(),
            target.clone(),
            new_version.clone(),
        )?;
        let new_remapping_state =
            CommitRemappingState::new(changesets_to_merge.clone(), new_version);

        // Diff the configs to find out action items.
        let diff = diff_configs(
            &old_config,
            &old_remapping_state.latest_synced_changesets,
            &new_config,
            &new_remapping_state.latest_synced_changesets,
        )?;

        // Construct the commit containing all the new content coming due to
        // config change.
        let additions_merge_cs_id = self
            .create_commit_with_new_sources(
                ctx,
                &target_repo,
                &diff,
                &changesets_to_merge,
                new_config.version.clone(),
                message.clone(),
                self.mutable_renames,
            )
            .await?;
        let additions_merge = if let Some(additions_merge_cs_id) = additions_merge_cs_id {
            let mut scuba = ctx.scuba().clone();
            scuba.log_with_msg(
                "Created change target config merge commit for addtions",
                Some(format!("{}", &additions_merge_cs_id)),
            );
            target_repo
                .changeset(additions_merge_cs_id)
                .await
                .map_err(MegarepoError::internal)?
        } else {
            None
        };

        // Construct the commit merging in all the new additions and removing no
        // longer needed content.
        let final_merge = self
            .create_final_merge_commit_with_removals(
                ctx,
                &target_repo,
                &diff.removed,
                message,
                &additions_merge,
                old_target_cs,
                &new_remapping_state,
                Some(new_config.version),
            )
            .await?;
        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg(
            "Created change target config merge commit connecting addtions to removals",
            Some(format!("{}", &final_merge)),
        );

        // Derrive all the necessary data before moving the bookmark
        let derived_data_types = target_repo
            .blob_repo()
            .get_active_derived_data_types_config()
            .types
            .iter();

        let derivers = FuturesUnordered::new();
        for ty in derived_data_types {
            let utils = derived_data_utils(ctx.fb, target_repo.blob_repo(), ty)?;
            derivers.push(utils.derive(ctx.clone(), target_repo.blob_repo().clone(), final_merge));
        }
        derivers.try_for_each(|_| future::ready(Ok(()))).await?;

        // Move bookmark
        self.move_bookmark_conditionally(
            ctx,
            target_repo.blob_repo(),
            target_bookmark.to_string(),
            (target_location, final_merge),
        )
        .await?;

        Ok(final_merge)
    }

    /// For all newly added sources and new versions of those already existing,
    /// applies the right move transformations and merges them all together into
    /// a single commit containing all the new stuff ready to be merged into the
    /// target.
    async fn create_commit_with_new_sources(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        diff: &SyncTargetConfigChanges,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        sync_config_version: SyncConfigVersion,
        message: Option<String>,
        mutable_renames: &Arc<MutableRenames>,
    ) -> Result<Option<ChangesetId>, MegarepoError> {
        if diff.added.is_empty() {
            return Ok(None);
        }

        let sources_to_add: Vec<_> = diff
            .added
            .iter()
            .map(|(source, _cs_id)| source.clone())
            .collect();
        let moved_commits = self
            .create_move_commits(
                ctx,
                repo.blob_repo(),
                &sources_to_add,
                changesets_to_merge,
                mutable_renames,
            )
            .await?;

        if moved_commits.len() == 1 {
            return Ok(Some(moved_commits[0].1.moved.get_changeset_id()));
        }

        // Now let's merge all the moved commits together
        Ok(Some(
            self.create_merge_commits(
                ctx,
                repo.blob_repo(),
                moved_commits,
                false, /* write_commit_remapping_state */
                sync_config_version,
                message,
            )
            .await?,
        ))
    }

    // If that change_target_config() call was successful, but failed to send
    // successful result to the client (e.g. network issues) then
    // client will retry a request. We need to detect this situation and
    // send a successful response to the client.
    async fn check_if_this_method_has_already_succeeded(
        &self,
        ctx: &CoreContext,
        new_version: &SyncConfigVersion,
        (expected_target_location, actual_target_location): (ChangesetId, ChangesetId),
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        repo: &RepoContext,
    ) -> Result<ChangesetId, MegarepoError> {
        // Bookmark points a non-expected commit - let's see if changeset it points to was created
        // by a previous change_target_config call

        // Check that first parent is a target location
        let parents = repo
            .blob_repo()
            .get_changeset_parents_by_bonsai(ctx.clone(), actual_target_location)
            .await?;
        if parents.get(0) != Some(&expected_target_location) {
            return Err(MegarepoError::request(anyhow!(
                "Neither {} nor its first parent {:?} point to a target location {}",
                actual_target_location,
                parents.get(0),
                expected_target_location,
            )));
        }

        self.check_if_commit_has_expected_remapping_state(
            ctx,
            actual_target_location,
            new_version,
            changesets_to_merge,
            repo,
        )
        .await?;

        Ok(actual_target_location)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::megarepo_test_utils::SyncTargetConfigBuilder;
    use anyhow::Error;
    use maplit::btreemap;
    use megarepo_config::Target;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    fn source_names(sources: &[(Source, ChangesetId)]) -> Vec<String> {
        sources
            .iter()
            .map(|(source, _cs_id)| source.source_name.clone())
            .collect()
    }

    #[test]
    fn test_diff_configs() -> Result<(), Error> {
        let repo_id = RepositoryId::new(1);
        let target = Target {
            repo_id: repo_id.id() as i64,
            bookmark: "target".to_string(),
        };

        let removed_source = SourceName::new("removed_source");
        let added_source = SourceName::new("added_source");
        let unchanged_source = SourceName::new("unchanged_source");
        let changed_source = SourceName::new("changed_source");
        let version_old = "version_old".to_string();
        let version_new = "version_old".to_string();
        let config_old = SyncTargetConfigBuilder::new(repo_id, target.clone(), version_old)
            .source_builder(removed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(unchanged_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(changed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .no_storage_build();

        let old_changesets = btreemap! {
            removed_source => ONES_CSID,
            changed_source.clone() =>ONES_CSID,
            unchanged_source.clone() =>ONES_CSID,
        };

        let config_new = SyncTargetConfigBuilder::new(repo_id, target, version_new)
            .source_builder(added_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(unchanged_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(changed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .linkfile("first", "linkfiles/first")
            .build_source()?
            .no_storage_build();

        let new_changesets = btreemap! {
            added_source => TWOS_CSID,
            changed_source => THREES_CSID,
            unchanged_source => ONES_CSID,
        };

        let diff = diff_configs(&config_old, &old_changesets, &config_new, &new_changesets)?;

        assert_eq!(
            source_names(&diff.added),
            vec!["added_source", "changed_source"]
        );
        assert_eq!(
            source_names(&diff.removed),
            vec!["changed_source", "removed_source"]
        );
        Ok(())
    }
}
