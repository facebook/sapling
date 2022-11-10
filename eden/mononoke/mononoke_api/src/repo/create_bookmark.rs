/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks_movement::CreateBookmarkOp;
use bytes::Bytes;
use hooks::HookManagerRef;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use skiplist::SkiplistIndexArc;
use tunables::tunables;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

impl RepoContext {
    /// Create a bookmark.
    pub async fn create_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        target: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        fn make_create_op<'a>(
            bookmark: &'a BookmarkName,
            target: ChangesetId,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> CreateBookmarkOp<'a> {
            let mut op = CreateBookmarkOp::new(bookmark, target, BookmarkUpdateReason::ApiRequest)
                .with_pushvars(pushvars);
            if !tunables().get_disable_commit_scribe_logging_scs() {
                op = op.log_new_public_commits_to_scribe();
            }
            op
        }
        if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(&bookmark).await?;
            if large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot create shared bookmark '{}' from small repo",
                    bookmark
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
                    &(redirector.repo.skiplist_index_arc() as Arc<dyn LeastCommonAncestorsHint>),
                    redirector.repo.hook_manager(),
                )
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.backsync_latest(ctx).await?;
        } else {
            make_create_op(&bookmark, target, pushvars)
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    self.inner_repo(),
                    &(self.skiplist_index_arc() as Arc<dyn LeastCommonAncestorsHint>),
                    self.hook_manager().as_ref(),
                )
                .await?;
        }

        Ok(())
    }
}
