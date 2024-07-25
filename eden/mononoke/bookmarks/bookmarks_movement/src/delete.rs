/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
use repo_authorization::RepoWriteOperation;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;

use crate::repo_lock::check_repo_lock;
use crate::restrictions::check_bookmark_sync_config;
use crate::restrictions::BookmarkKindRestrictions;
use crate::BookmarkInfoData;
use crate::BookmarkInfoTransaction;
use crate::BookmarkMovementError;
use crate::Repo;
use crate::ALLOW_NON_FFWD_PUSHVAR;

#[must_use = "DeleteBookmarkOp must be run to have an effect"]
pub struct DeleteBookmarkOp<'op> {
    bookmark: BookmarkKey,
    old_target: ChangesetId,
    reason: BookmarkUpdateReason,
    kind_restrictions: BookmarkKindRestrictions,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    only_log_acl_checks: bool,
}

impl<'op> DeleteBookmarkOp<'op> {
    pub fn new(
        bookmark: BookmarkKey,
        old_target: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> DeleteBookmarkOp<'op> {
        DeleteBookmarkOp {
            bookmark,
            old_target,
            reason,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            pushvars: None,
            only_log_acl_checks: false,
        }
    }

    pub fn only_if_scratch(mut self) -> Self {
        self.kind_restrictions = BookmarkKindRestrictions::OnlyScratch;
        self
    }

    pub fn only_if_public(mut self) -> Self {
        self.kind_restrictions = BookmarkKindRestrictions::OnlyPublishing;
        self
    }

    pub fn with_pushvars(mut self, pushvars: Option<&'op HashMap<String, Bytes>>) -> Self {
        self.pushvars = pushvars;
        self
    }

    pub fn only_log_acl_checks(mut self, only_log: bool) -> Self {
        self.only_log_acl_checks = only_log;
        self
    }

    pub async fn run_with_transaction(
        self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
        txn: Option<Box<dyn BookmarkTransaction>>,
    ) -> Result<BookmarkInfoTransaction, BookmarkMovementError> {
        let kind = self.kind_restrictions.check_kind(repo, &self.bookmark)?;

        if self.only_log_acl_checks {
            if authz
                .check_repo_write(ctx, repo, RepoWriteOperation::DeleteBookmark(kind))
                .await
                .is_denied()
            {
                ctx.scuba()
                    .clone()
                    .log_with_msg("Repo write ACL check would fail for bookmark delete", None);
            }
        } else {
            authz
                .require_repo_write(ctx, repo, RepoWriteOperation::DeleteBookmark(kind))
                .await?;
        }
        authz
            .require_bookmark_modify(ctx, repo, &self.bookmark)
            .await?;

        check_bookmark_sync_config(ctx, repo, &self.bookmark, kind).await?;
        let fast_forward_only = repo
            .repo_bookmark_attrs()
            .is_fast_forward_only(&self.bookmark);
        let bypass = self.pushvars.map_or(false, |pushvar| {
            pushvar.contains_key(ALLOW_NON_FFWD_PUSHVAR)
        });
        if fast_forward_only && !bypass {
            // Cannot delete fast-forward-only bookmarks.
            return Err(BookmarkMovementError::DeletionProhibited {
                bookmark: self.bookmark.clone(),
            });
        }

        check_repo_lock(
            repo,
            kind,
            self.pushvars,
            ctx.metadata().identities(),
            authz,
        )
        .await?;

        ctx.scuba()
            .clone()
            .add("bookmark", self.bookmark.to_string())
            .log_with_msg("Deleting bookmark", None);
        let mut txn = txn.unwrap_or_else(|| repo.bookmarks().create_transaction(ctx.clone()));
        match kind {
            BookmarkKind::Scratch => {
                txn.delete_scratch(&self.bookmark, self.old_target)?;
            }
            BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
                txn.delete(&self.bookmark, self.old_target, self.reason)?;
            }
        }
        let info = BookmarkInfo {
            bookmark_name: self.bookmark.clone(),
            bookmark_kind: kind,
            operation: BookmarkOperation::Delete(self.old_target),
            reason: self.reason,
        };
        let info_data = BookmarkInfoData::new(info, false, vec![]);
        Ok(BookmarkInfoTransaction::new(info_data, txn, vec![]))
    }

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
    ) -> Result<BookmarkUpdateLogId, BookmarkMovementError> {
        let info_txn = self.run_with_transaction(ctx, authz, repo, None).await?;
        info_txn.commit_and_log(ctx, repo).await
    }
}
