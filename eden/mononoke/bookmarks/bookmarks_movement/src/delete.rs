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
use metaconfig_types::BookmarkAttrs;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::SourceControlServiceParams;
use mononoke_types::ChangesetId;
use repo_read_write_status::RepoReadWriteFetcher;

use crate::repo_lock::check_repo_lock;
use crate::restrictions::check_bookmark_sync_config;
use crate::restrictions::BookmarkKindRestrictions;
use crate::restrictions::BookmarkMoveAuthorization;
use crate::BookmarkMovementError;
use crate::Repo;

#[must_use = "DeleteBookmarkOp must be run to have an effect"]
pub struct DeleteBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    old_target: ChangesetId,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization<'op>,
    kind_restrictions: BookmarkKindRestrictions,
    pushvars: Option<&'op HashMap<String, Bytes>>,
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
            auth: BookmarkMoveAuthorization::User,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            pushvars: None,
        }
    }

    /// This bookmark change is for an authenticated named service.  The change
    /// will be checked against the service's write restrictions.
    pub fn for_service(
        mut self,
        service_name: impl Into<String>,
        params: &'op SourceControlServiceParams,
    ) -> Self {
        self.auth = BookmarkMoveAuthorization::Service(service_name.into(), params);
        self
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

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        repo: &'op impl Repo,
        infinitepush_params: &'op InfinitepushParams,
        bookmark_attrs: &'op BookmarkAttrs,
        repo_read_write_fetcher: &'op RepoReadWriteFetcher,
    ) -> Result<(), BookmarkMovementError> {
        let kind = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)
            .await?;

        check_bookmark_sync_config(repo, self.bookmark, kind)?;

        if bookmark_attrs.is_fast_forward_only(self.bookmark) {
            // Cannot delete fast-forward-only bookmarks.
            return Err(BookmarkMovementError::DeletionProhibited {
                bookmark: self.bookmark.clone(),
            });
        }

        check_repo_lock(
            repo_read_write_fetcher,
            kind,
            self.pushvars,
            repo.repo_permission_checker(),
            ctx.metadata().identities(),
        )
        .await?;

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
