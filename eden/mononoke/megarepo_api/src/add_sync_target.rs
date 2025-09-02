/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use context::CoreContext;
use futures::TryFutureExt;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::SyncTargetConfig;
use megarepo_config::verify_config;
use megarepo_error::MegarepoError;
use megarepo_mapping::SourceName;
use metaconfig_types::RepoConfigArc;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;

use crate::common::MegarepoOp;
use crate::common::derive_all_types;

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
pub struct AddSyncTarget<'a, R> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke<R>>,
}

impl<'a, R> MegarepoOp<R> for AddSyncTarget<'a, R> {
    fn mononoke(&self) -> &Arc<Mononoke<R>> {
        self.mononoke
    }
}

impl<'a, R: MononokeRepo> AddSyncTarget<'a, R> {
    pub fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke<R>>,
    ) -> Self {
        Self {
            megarepo_configs,
            mononoke,
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
                repo.repo(),
                &sync_target_config.sources,
                &changesets_to_merge,
            )
            .await?;
        scuba.log_with_msg("Created move commits", None);

        // Now let's merge all the moved commits together
        let top_merge_cs_id = self
            .create_merge_commits(
                ctx,
                repo.repo(),
                moved_commits,
                true, /* write_commit_remapping_state */
                &sync_target_config,
                message,
                sync_target_config.target.bookmark.clone(),
            )
            .await?;

        scuba.log_with_msg(
            "Created add sync target merge commit",
            Some(format!("{}", top_merge_cs_id)),
        );

        derive_all_types(ctx, repo.repo(), &[top_merge_cs_id]).await?;

        scuba.log_with_msg("Derived data", None);

        let repo_config = repo.repo().repo_config_arc();
        self.megarepo_configs
            .add_config_version(ctx.clone(), repo_config, sync_target_config.clone())
            .await?;

        self.create_bookmark(
            ctx,
            repo.repo(),
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
        repo: &RepoContext<R>,
    ) -> Result<Option<ChangesetId>, MegarepoError> {
        let bookmark_name = &sync_target_config.target.bookmark;
        let bookmark = BookmarkKey::new(bookmark_name).map_err(MegarepoError::request)?;

        let maybe_cs_id = repo
            .repo()
            .bookmarks()
            .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
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
