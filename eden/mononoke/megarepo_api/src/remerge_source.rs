/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::find_source_config;
use crate::common::find_target_bookmark_and_value;
use crate::common::find_target_sync_config;
use crate::common::MegarepoOp;
use anyhow::anyhow;
use context::CoreContext;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_mapping::SourceName;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mutable_renames::MutableRenames;
use std::sync::Arc;

// remerge_source resets source in a given target to a specified commit.
// This is normally used for the cases where a bookmark had a non-fast
// forward move.
pub struct RemergeSource<'a> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke>,
    pub mutable_renames: &'a Arc<MutableRenames>,
}

impl<'a> MegarepoOp for RemergeSource<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        self.mononoke
    }
}

impl<'a> RemergeSource<'a> {
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
        source_name: &SourceName,
        remerge_cs_id: ChangesetId,
        message: Option<String>,
        target: &Target,
        target_location: ChangesetId,
    ) -> Result<ChangesetId, MegarepoError> {
        let target_repo = self.find_repo_by_id(ctx, target.repo_id).await?;

        // Find the target config version and remapping state that was used to
        // create the latest target commit.
        let (_, actual_target_location) =
            find_target_bookmark_and_value(ctx, &target_repo, target).await?;

        // target doesn't point to the commit we expect - check
        // if this method has already succeded and just immediately return the
        // result if so.
        if actual_target_location != target_location {
            return self
                .check_if_this_method_has_already_succeeded(
                    ctx,
                    (target_location, actual_target_location),
                    source_name,
                    remerge_cs_id,
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
        let (old_remapping_state, config) = find_target_sync_config(
            ctx,
            target_repo.blob_repo(),
            target_location,
            target,
            self.megarepo_configs,
        )
        .await?;

        let mut new_remapping_state = old_remapping_state.clone();
        new_remapping_state.set_source_changeset(source_name.clone(), remerge_cs_id);

        let source_config = find_source_config(source_name, &config)?;

        let moved_commits = self
            .create_move_commits(
                ctx,
                target_repo.blob_repo(),
                &[source_config.clone()],
                new_remapping_state.get_all_latest_synced_changesets(),
                self.mutable_renames,
            )
            .await?;

        if moved_commits.len() != 1 {
            return Err(
                anyhow!("unexpected number of move commits {}", moved_commits.len()).into(),
            );
        }

        let move_commit = &moved_commits[0];
        let move_commit = target_repo
            .changeset(move_commit.1.moved.get_changeset_id())
            .await?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!("programming error - moved changeset not found!"))
            })?;

        let current_source_cs = old_remapping_state
            .get_latest_synced_changeset(source_name)
            .ok_or_else(|| {
                anyhow!(
                    "Source {} does not exist in target {:?}",
                    source_name,
                    target
                )
            })?;

        let remerged = self
            .create_final_merge_commit_with_removals(
                ctx,
                &target_repo,
                &[(source_config.clone(), *current_source_cs)],
                message,
                &Some(move_commit),
                old_target_cs,
                &new_remapping_state,
                None, // new_version parameter. Since version doesn't change let's pass None here
            )
            .await?;

        self.move_bookmark_conditionally(
            ctx,
            target_repo.blob_repo(),
            target.bookmark.clone(),
            (target_location, remerged),
        )
        .await?;

        Ok(remerged)
    }

    async fn check_if_this_method_has_already_succeeded(
        &self,
        ctx: &CoreContext,
        (expected_target_location, actual_target_location): (ChangesetId, ChangesetId),
        source_name: &SourceName,
        remerge_cs_id: ChangesetId,
        repo: &RepoContext,
    ) -> Result<ChangesetId, MegarepoError> {
        let parents = repo
            .blob_repo()
            .get_changeset_parents_by_bonsai(ctx.clone(), actual_target_location)
            .await?;
        if parents.len() != 2 || parents[0] != expected_target_location {
            return Err(MegarepoError::request(anyhow!(
                "Neither {} nor its first parent {:?} point to a target location {}",
                actual_target_location,
                parents.get(0),
                expected_target_location,
            )));
        }

        let state = self
            .read_remapping_state_file(ctx, repo, actual_target_location)
            .await?;
        let latest_synced_for_source = state.get_latest_synced_changeset(source_name);
        if state.get_latest_synced_changeset(source_name) != Some(&remerge_cs_id) {
            return Err(MegarepoError::request(anyhow!(
                "Target cs {} has unexpected changeset {:?} for {}",
                actual_target_location,
                latest_synced_for_source,
                source_name,
            )));
        }

        Ok(actual_target_location)
    }
}
