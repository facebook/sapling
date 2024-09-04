/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::format_err;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use bookmarks_movement::BookmarkInfoTransaction;
use bookmarks_movement::CreateBookmarkOp;
use bytes::Bytes;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use hook_manager::manager::HookManagerRef;
use mononoke_types::ChangesetId;

use crate::errors::MononokeError;
use crate::invalid_push_redirected_request;
use crate::repo::RepoContext;
use crate::MononokeRepo;

impl<R: MononokeRepo> RepoContext<R> {
    async fn create_bookmark_op<'a>(
        &self,
        bookmark: &'_ BookmarkKey,
        target: ChangesetId,
        pushvars: Option<&'a HashMap<String, Bytes>>,
    ) -> Result<CreateBookmarkOp<'a>, MononokeError> {
        self.start_write()?;

        fn make_create_op<'a>(
            bookmark: &'_ BookmarkKey,
            target: ChangesetId,
            pushvars: Option<&'a HashMap<String, Bytes>>,
        ) -> CreateBookmarkOp<'a> {
            let op =
                CreateBookmarkOp::new(bookmark.clone(), target, BookmarkUpdateReason::ApiRequest)
                    .with_pushvars(pushvars);
            op.log_new_public_commits_to_scribe()
        }
        let create_op = if let Some(redirector) = self.push_redirector.as_ref() {
            let large_bookmark = redirector.small_to_large_bookmark(bookmark).await?;
            if &large_bookmark == bookmark {
                return Err(MononokeError::InvalidRequest(format!(
                    "Cannot create shared bookmark '{}' from small repo",
                    bookmark.name()
                )));
            }
            let ctx = self.ctx();
            let target = redirector
                .small_to_large_commit_syncer
                .sync_commit(
                    ctx,
                    target,
                    CandidateSelectionHint::Only,
                    CommitSyncContext::PushRedirector,
                    false,
                )
                .await?
                .ok_or_else(|| {
                    format_err!(
                        "Error in create_bookmark absence of corresponding commit in target repo for {}",
                        target,
                    )
                })?;
            make_create_op(&large_bookmark, target, pushvars)
        } else {
            make_create_op(bookmark, target, pushvars)
        };
        Ok(create_op)
    }

    /// Create a bookmark.
    pub async fn create_bookmark(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        let create_op = self.create_bookmark_op(bookmark, target, pushvars).await?;
        if let Some(redirector) = self.push_redirector.as_ref() {
            let ctx = self.ctx();
            let log_id = create_op
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
            create_op
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

    /// Create a bookmark with provided transaction.
    pub async fn create_bookmark_with_transaction(
        &self,
        bookmark: &BookmarkKey,
        target: ChangesetId,
        pushvars: Option<&HashMap<String, Bytes>>,
        txn: Option<Box<dyn BookmarkTransaction>>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Result<BookmarkInfoTransaction, MononokeError> {
        if self.push_redirector.is_some() {
            return Err(invalid_push_redirected_request(
                "create_bookmark_with_transaction",
            ));
        }
        let create_op = self.create_bookmark_op(bookmark, target, pushvars).await?;
        let bookmark_info_transaction = create_op
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
