/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks_movement::CreateBookmarkOp;
use bytes::Bytes;
use hook_manager::manager::HookManagerRef;
use mononoke_types::ChangesetId;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

impl RepoContext {
    /// Create a bookmark.
    pub async fn create_bookmark(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        fn make_create_op<'a>(
            bookmark: &'a BookmarkKey,
            target: ChangesetId,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> CreateBookmarkOp<'a> {
            let op = CreateBookmarkOp::new(bookmark, target, BookmarkUpdateReason::ApiRequest)
                .with_pushvars(pushvars);
            op.log_new_public_commits_to_scribe()
        }
        if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(bookmark).await?;
            if &large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot create shared bookmark '{}' from small repo",
                    bookmark.name()
                )));
            }
            let ctx = self.ctx();
            let target = redirector
                .get_small_to_large_commit_equivalent(ctx, target)
                .await?;
            make_create_op(&large_bookmark, target, pushvars)
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    redirector.repo.inner_repo(),
                    redirector.repo.hook_manager(),
                )
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.backsync_latest(ctx).await?;
        } else {
            make_create_op(bookmark, target, pushvars)
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    self.inner_repo(),
                    self.hook_manager().as_ref(),
                )
                .await?;
        }
        Ok(())
    }
}
