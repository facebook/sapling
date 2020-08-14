/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use bookmarks::{BookmarkUpdateReason, BundleReplay};
use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams};
use mononoke_types::ChangesetId;

use crate::{BookmarkKindRestrictions, BookmarkMoveAuthorization, BookmarkMovementError};

pub struct DeleteBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    old_target: ChangesetId,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization,
    kind_restrictions: BookmarkKindRestrictions,
    bundle_replay: Option<&'op dyn BundleReplay>,
}

#[must_use = "DeleteBookmarkOp must be run to have an effect"]
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
            auth: BookmarkMoveAuthorization::Context,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            bundle_replay: None,
        }
    }

    pub fn only_if_scratch(mut self) -> Self {
        self.kind_restrictions = BookmarkKindRestrictions::OnlyScratch;
        self
    }

    pub fn only_if_public(mut self) -> Self {
        self.kind_restrictions = BookmarkKindRestrictions::OnlyPublic;
        self
    }

    pub fn with_bundle_replay_data(mut self, bundle_replay: Option<&'op dyn BundleReplay>) -> Self {
        self.bundle_replay = bundle_replay;
        self
    }

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        repo: &'op BlobRepo,
        infinitepush_params: &'op InfinitepushParams,
        bookmark_attrs: &'op BookmarkAttrs,
    ) -> Result<(), BookmarkMovementError> {
        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)?;

        let is_scratch = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        if is_scratch || bookmark_attrs.is_fast_forward_only(self.bookmark) {
            // Cannot delete scratch or fast-forward-only bookmarks.
            return Err(BookmarkMovementError::DeletionProhibited {
                bookmark: self.bookmark.clone(),
            });
        }

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        txn.delete(
            self.bookmark,
            self.old_target,
            self.reason,
            self.bundle_replay,
        )?;

        let ok = txn.commit().await?;
        if !ok {
            return Err(BookmarkMovementError::TransactionFailed);
        }

        Ok(())
    }
}
