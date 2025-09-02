/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks_movement::BookmarkInfoTransaction;
use bookmarks_movement::DeleteBookmarkOp;
use bytes::Bytes;
use mononoke_types::ChangesetId;

use crate::MononokeRepo;
use crate::errors::MononokeError;
use crate::invalid_push_redirected_request;
use crate::repo::RepoContext;

impl<R: MononokeRepo> RepoContext<R> {
    async fn delete_bookmark_op<'a>(
        &self,
        bookmark: &'_ BookmarkKey,
        old_target: Option<ChangesetId>,
        pushvars: Option<&'a HashMap<String, Bytes>>,
    ) -> Result<DeleteBookmarkOp<'a>, MononokeError> {
        self.start_write()?;

        // We need to find out where the bookmark currently points to in order
        // to delete it.  Make sure to bypass any out-of-date caches.
        let old_target = match old_target {
            Some(old_target) => old_target,
            None => self
                .repo()
                .bookmarks()
                .get(
                    self.ctx().clone(),
                    bookmark,
                    bookmarks::Freshness::MostRecent,
                )
                .await
                .context("Failed to fetch old bookmark target")?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("bookmark '{}' does not exist", bookmark))
                })?,
        };

        fn make_delete_op<'a>(
            bookmark: &'_ BookmarkKey,
            old_target: ChangesetId,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> DeleteBookmarkOp<'a> {
            DeleteBookmarkOp::new(
                bookmark.clone(),
                old_target,
                BookmarkUpdateReason::ApiRequest,
            )
            .with_pushvars(pushvars)
        }
        let delete_op = if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(bookmark).await?;
            if &large_bookmark == bookmark {
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
        } else {
            make_delete_op(bookmark, old_target, pushvars)
        };
        Ok(delete_op)
    }

    /// Delete a bookmark.
    pub async fn delete_bookmark(
        &self,
        bookmark: &BookmarkKey,
        old_target: Option<ChangesetId>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        let delete_op = self
            .delete_bookmark_op(bookmark, old_target, pushvars)
            .await?;
        if let Some(redirector) = self.push_redirector.as_ref() {
            let ctx = self.ctx();
            let log_id = delete_op
                .run(self.ctx(), self.authorization_context(), &redirector.repo)
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.ensure_backsynced(ctx, log_id).await?;
        } else {
            delete_op
                .run(self.ctx(), self.authorization_context(), self.repo())
                .await?;
        }
        Ok(())
    }

    /// Delete a bookmark with provided transaction.
    pub async fn delete_bookmark_with_transaction(
        &self,
        bookmark: &BookmarkKey,
        old_target: Option<ChangesetId>,
        pushvars: Option<&HashMap<String, Bytes>>,
        txn: Option<Box<dyn BookmarkTransaction>>,
    ) -> Result<BookmarkInfoTransaction, MononokeError> {
        if self.push_redirector.is_some() {
            return Err(invalid_push_redirected_request(
                "delete_bookmark_with_transaction",
            ));
        }
        let delete_op = self
            .delete_bookmark_op(bookmark, old_target, pushvars)
            .await?;

        let bookmark_info_transaction = delete_op
            .run_with_transaction(self.ctx(), self.authorization_context(), self.repo(), txn)
            .await?;
        Ok(bookmark_info_transaction)
    }
}
