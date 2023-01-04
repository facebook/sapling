/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks_movement::DeleteBookmarkOp;
use bytes::Bytes;
use mononoke_types::ChangesetId;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

impl RepoContext {
    /// Delete a bookmark.
    pub async fn delete_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        old_target: Option<ChangesetId>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        // We need to find out where the bookmark currently points to in order
        // to delete it.  Make sure to bypass any out-of-date caches.
        let old_target = match old_target {
            Some(old_target) => old_target,
            None => self
                .blob_repo()
                .bookmarks()
                .get(self.ctx().clone(), &bookmark)
                .await
                .context("Failed to fetch old bookmark target")?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("bookmark '{}' does not exist", bookmark))
                })?,
        };

        fn make_delete_op<'a>(
            bookmark: &'a BookmarkName,
            old_target: ChangesetId,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> DeleteBookmarkOp<'a> {
            DeleteBookmarkOp::new(bookmark, old_target, BookmarkUpdateReason::ApiRequest)
                .with_pushvars(pushvars)
        }
        if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(&bookmark).await?;
            if large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot delete shared bookmark '{}' from small repo",
                    bookmark
                )));
            }
            let ctx = self.ctx();
            let old_target = redirector
                .get_small_to_large_commit_equivalent(ctx, old_target)
                .await?;
            make_delete_op(&large_bookmark, old_target, pushvars)
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    redirector.repo.inner_repo(),
                )
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.backsync_latest(ctx).await?;
        } else {
            make_delete_op(&bookmark, old_target, pushvars)
                .run(self.ctx(), self.authorization_context(), self.inner_repo())
                .await?;
        }

        Ok(())
    }
}
