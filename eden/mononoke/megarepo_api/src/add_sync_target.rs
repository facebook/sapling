/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{MegarepoOp, SourceName};
use context::CoreContext;
use derived_data_utils::derived_data_utils;
use futures::{future, stream::FuturesUnordered, TryStreamExt};
use megarepo_config::{verify_config, MononokeMegarepoConfigs, SyncTargetConfig};
use megarepo_error::MegarepoError;
use mononoke_api::Mononoke;
use mononoke_types::ChangesetId;
use std::{collections::HashMap, sync::Arc};

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
}

impl<'a> MegarepoOp for AddSyncTarget<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        &self.mononoke
    }
}

impl<'a> AddSyncTarget<'a> {
    pub fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke>,
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
        changesets_to_merge: HashMap<SourceName, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        verify_config(ctx, &sync_target_config).map_err(MegarepoError::request)?;

        let repo = self
            .find_repo_by_id(ctx, sync_target_config.target.repo_id)
            .await?;

        // First let's create commit on top of all source commits that
        // move all files in a correct place
        let moved_commits = self
            .create_move_commits(
                ctx,
                repo.blob_repo(),
                &sync_target_config.sources,
                &changesets_to_merge,
            )
            .await?;

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

        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg(
            "Created add sync target merge commit",
            Some(format!("{}", top_merge_cs_id)),
        );

        let derived_data_types = repo
            .blob_repo()
            .get_derived_data_config()
            .enabled
            .types
            .iter();

        let derivers = FuturesUnordered::new();
        for ty in derived_data_types {
            let utils = derived_data_utils(repo.blob_repo(), ty)?;
            derivers.push(utils.derive(ctx.clone(), repo.blob_repo().clone(), top_merge_cs_id));
        }

        derivers.try_for_each(|_| future::ready(Ok(()))).await?;

        self.megarepo_configs
            .add_target_with_config_version(ctx.clone(), sync_target_config.clone())
            .await?;

        self.move_bookmark(
            ctx,
            repo.blob_repo(),
            sync_target_config.target.bookmark,
            top_merge_cs_id,
        )
        .await?;

        Ok(top_merge_cs_id)
    }
}
