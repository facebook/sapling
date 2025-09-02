/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::format_err;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks_movement::BookmarkInfoTransaction;
use bookmarks_movement::BookmarkUpdatePolicy;
use bookmarks_movement::BookmarkUpdateTargets;
use bookmarks_movement::UpdateBookmarkOp;
use bytes::Bytes;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::sync_commit;
use hook_manager::manager::HookManagerRef;
use mononoke_types::ChangesetId;

use crate::MononokeRepo;
use crate::errors::MononokeError;
use crate::invalid_push_redirected_request;
use crate::repo::RepoContext;

impl<R: MononokeRepo> RepoContext<R> {
    /// Create operation for moving a bookmark
    async fn move_bookmark_op<'a>(
        &self,
        bookmark: &'_ BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&'a HashMap<String, Bytes>>,
    ) -> Result<UpdateBookmarkOp<'a>, MononokeError> {
        self.start_write()?;

        // We need to find out where the bookmark currently points to in order
        // to move it.  Make sure to bypass any out-of-date caches.
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

        fn make_move_op<'a>(
            bookmark: &'_ BookmarkKey,
            target: ChangesetId,
            old_target: ChangesetId,
            allow_non_fast_forward: bool,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> UpdateBookmarkOp<'a> {
            let op = UpdateBookmarkOp::new(
                bookmark.clone(),
                BookmarkUpdateTargets {
                    old: old_target,
                    new: target,
                },
                if allow_non_fast_forward {
                    BookmarkUpdatePolicy::AnyPermittedByConfig
                } else {
                    BookmarkUpdatePolicy::FastForwardOnly
                },
                BookmarkUpdateReason::ApiRequest,
            )
            .with_pushvars(pushvars);
            op.log_new_public_commits_to_scribe()
        }
        let op = if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(bookmark).await?;
            if &large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot move shared bookmark '{}' from small repo",
                    bookmark
                )));
            }
            let ctx = self.ctx();
            let target = sync_commit(
                ctx,
                target,
                &redirector.small_to_large_commit_syncer,
                CandidateSelectionHint::Only,
                CommitSyncContext::PushRedirector,
                false,
            )
            .await?
            .ok_or_else(|| {
                format_err!(
                    "Error in move_bookmark absence of corresponding commit in target repo for {}",
                    target,
                )
            })?;
            let old_target = redirector
                .get_small_to_large_commit_equivalent(ctx, old_target)
                .await?;
            make_move_op(
                &large_bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
        } else {
            make_move_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
        };
        Ok(op)
    }

    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        let update_op = self
            .move_bookmark_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
            .await?;
        if let Some(redirector) = self.push_redirector.as_ref() {
            let ctx = self.ctx();
            let log_id = update_op
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    &redirector.repo,
                    redirector.repo.hook_manager(),
                )
                .await?;
            // Wait for bookmark to catch up on small repo
            redirector.ensure_backsynced(ctx, log_id).await?;
        } else {
            update_op
                .run(
                    self.ctx(),
                    self.authorization_context(),
                    self.repo(),
                    self.hook_manager().as_ref(),
                )
                .await?;
        }
        Ok(())
    }

    /// Move a bookmark with provided transaction
    pub async fn move_bookmark_with_transaction(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        old_target: Option<ChangesetId>,
        allow_non_fast_forward: bool,
        pushvars: Option<&HashMap<String, Bytes>>,
        txn: Option<Box<dyn BookmarkTransaction>>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Result<BookmarkInfoTransaction, MononokeError> {
        if self.push_redirector.is_some() {
            return Err(invalid_push_redirected_request(
                "move_bookmark_with_transaction",
            ));
        }
        let update_op = self
            .move_bookmark_op(
                bookmark,
                target,
                old_target,
                allow_non_fast_forward,
                pushvars,
            )
            .await?;
        let bookmark_info_transaction = update_op
            .run_with_transaction(
                self.ctx(),
                self.authorization_context(),
                self.repo(),
                self.hook_manager().as_ref(),
                txn,
                txn_hooks,
            )
            .await?;
        Ok(bookmark_info_transaction)
    }
}
