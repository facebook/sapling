/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::MegarepoOp;
use bookmarks::BookmarkName;
use context::CoreContext;
use derived_data_utils::derived_data_utils;
use futures::future;
use futures::stream::FuturesUnordered;
use futures::TryFutureExt;
use futures::TryStreamExt;
use megarepo_config::verify_config;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::SyncTargetConfig;
use megarepo_error::MegarepoError;
use megarepo_mapping::SourceName;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mutable_renames::MutableRenames;
use std::collections::BTreeMap;
use std::sync::Arc;

// Create a new sync target given a config.
// After this command finishes it creates
// move commits on top of source commits
// and also merges them all together.
//
//      Tn
//      | \
//     ...
//      |
//      T1
//     / \
//    M   M
//   /     \
//  S       S
//
// Tx - target merge commits
// M - move commits
// S - source commits that need to be merged
pub struct AddSyncTarget<'a> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke>,
    pub mutable_renames: &'a Arc<MutableRenames>,
}

impl<'a> MegarepoOp for AddSyncTarget<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        self.mononoke
    }
}

impl<'a> AddSyncTarget<'a> {
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
        sync_target_config: SyncTargetConfig,
        changesets_to_merge: BTreeMap<SourceName, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let mut scuba = ctx.scuba().clone();

        verify_config(ctx, &sync_target_config).map_err(MegarepoError::request)?;

        let repo = self
            .find_repo_by_id(ctx, sync_target_config.target.repo_id)
            .await?;

        let maybe_already_done = self
            .check_if_this_method_has_already_succeeded(
                ctx,
                &sync_target_config,
                &changesets_to_merge,
                &repo,
            )
            .await?;
        if let Some(already_done) = maybe_already_done {
            // The same request has already succeeded, nothing to do.
            return Ok(already_done);
        }

        // First let's create commit on top of all source commits that
        // move all files in a correct place
        let moved_commits = self
            .create_move_commits(
                ctx,
                repo.blob_repo(),
                &sync_target_config.sources,
                &changesets_to_merge,
                self.mutable_renames,
            )
            .await?;
        scuba.log_with_msg("Created move commits", None);

        // Now let's merge all the moved commits together
        let top_merge_cs_id = self
            .create_merge_commits(
                ctx,
                repo.blob_repo(),
                moved_commits,
                true, /* write_commit_remapping_state */
                sync_target_config.version.clone(),
                message,
            )
            .await?;

        scuba.log_with_msg(
            "Created add sync target merge commit",
            Some(format!("{}", top_merge_cs_id)),
        );

        // add_sync_target might need to derive a lot of data, and it takes a long time to
        // do it. We don't have any resumability, so if it fails for any reason, then we'd
        // need to start over.

        // For now let's just retry a few times so that we don't have to start over
        // because of flakiness
        let mut i = 0;
        loop {
            i += 1;
            let derived_data_types = repo
                .blob_repo()
                .get_active_derived_data_types_config()
                .types
                .iter();
            let derivers = FuturesUnordered::new();
            for ty in derived_data_types {
                let utils = derived_data_utils(ctx.fb, repo.blob_repo(), ty)?;
                derivers.push(utils.derive(ctx.clone(), repo.blob_repo().clone(), top_merge_cs_id));
            }

            let res = derivers.try_for_each(|_| future::ready(Ok(()))).await;
            match res {
                Ok(()) => {
                    break;
                }
                Err(err) => {
                    scuba.log_with_msg("Derived data failed, retrying", Some(format!("{:#}", err)));
                    if i >= 5 {
                        return Err(err.into());
                    }
                }
            }
        }

        scuba.log_with_msg("Derived data", None);

        self.megarepo_configs
            .add_config_version(ctx.clone(), sync_target_config.clone())
            .await?;

        self.create_bookmark(
            ctx,
            repo.blob_repo(),
            sync_target_config.target.bookmark,
            top_merge_cs_id,
        )
        .await?;

        Ok(top_merge_cs_id)
    }

    // If that add_sync_target() call was successful, but failed to send
    // successful result to the client (e.g. network issues) then
    // client will retry a request. We need to detect this situation and
    // send a successful response to the client.
    async fn check_if_this_method_has_already_succeeded(
        &self,
        ctx: &CoreContext,
        sync_target_config: &SyncTargetConfig,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        repo: &RepoContext,
    ) -> Result<Option<ChangesetId>, MegarepoError> {
        let bookmark_name = &sync_target_config.target.bookmark;
        let bookmark = BookmarkName::new(bookmark_name).map_err(MegarepoError::request)?;

        let maybe_cs_id = repo
            .blob_repo()
            .bookmarks()
            .get(ctx.clone(), &bookmark)
            .map_err(MegarepoError::internal)
            .await?;

        let cs_id = match maybe_cs_id {
            Some(cs_id) => cs_id,
            None => {
                // Bookmark just doesn't exist - proceed the method as planned
                return Ok(None);
            }
        };

        // Bookmark exists - let's see if changeset it points to was created
        // by a previous add_sync_target call

        // First let's check if that config from request is the same as what's stored
        // our config storage
        self.check_if_new_sync_target_config_is_equivalent_to_already_existing(
            ctx,
            self.megarepo_configs,
            sync_target_config,
        )
        .await?;

        self.check_if_commit_has_expected_remapping_state(
            ctx,
            cs_id,
            &sync_target_config.version,
            changesets_to_merge,
            repo,
        )
        .await?;

        Ok(Some(cs_id))
    }
}
