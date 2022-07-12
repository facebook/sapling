/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use metaconfig_types::InfinitepushParams;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
use repo_authorization::RepoWriteOperation;

use crate::repo_lock::check_repo_lock;
use crate::restrictions::check_bookmark_sync_config;
use crate::restrictions::BookmarkKindRestrictions;
use crate::BookmarkMovementError;
use crate::Repo;

#[must_use = "DeleteBookmarkOp must be run to have an effect"]
pub struct DeleteBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    old_target: ChangesetId,
    reason: BookmarkUpdateReason,
    kind_restrictions: BookmarkKindRestrictions,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    only_log_acl_checks: bool,
}

impl<'op> DeleteBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkName,
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

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
        infinitepush_params: &'op InfinitepushParams,
    ) -> Result<(), BookmarkMovementError> {
        let kind = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        if self.only_log_acl_checks {
            if authz
                .check_repo_write(ctx, repo, RepoWriteOperation::DeleteBookmark(kind))
                .await?
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
            .require_bookmark_modify(ctx, repo, self.bookmark)
            .await?;

        check_bookmark_sync_config(repo, self.bookmark, kind)?;

        if repo
            .repo_bookmark_attrs()
            .is_fast_forward_only(self.bookmark)
        {
            // Cannot delete fast-forward-only bookmarks.
            return Err(BookmarkMovementError::DeletionProhibited {
                bookmark: self.bookmark.clone(),
            });
        }

        check_repo_lock(repo, kind, self.pushvars, ctx.metadata().identities()).await?;

        ctx.scuba()
            .clone()
            .add("bookmark", self.bookmark.to_string())
            .log_with_msg("Deleting bookmark", None);
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        match kind {
            BookmarkKind::Scratch => {
                txn.delete_scratch(self.bookmark, self.old_target)?;
            }
            BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
                txn.delete(self.bookmark, self.old_target, self.reason)?;
            }
        }

        let ok = txn.commit().await?;
        if !ok {
            return Err(BookmarkMovementError::TransactionFailed);
        }

        Ok(())
    }
}
