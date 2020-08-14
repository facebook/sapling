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

pub struct CreateBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    target: ChangesetId,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization,
    kind_restrictions: BookmarkKindRestrictions,
    bundle_replay: Option<&'op dyn BundleReplay>,
}

#[must_use = "CreateBookmarkOp must be run to have an effect"]
impl<'op> CreateBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkName,
        target: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> CreateBookmarkOp<'op> {
        CreateBookmarkOp {
            bookmark,
            target,
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

        let mut txn = repo.update_bookmark_transaction(ctx.clone());

        if is_scratch {
            txn.create_scratch(self.bookmark, self.target)?;
        } else {
            unimplemented!("Non-scratch bookmark create");
            // txn.create(bookmark, target, reason, bundle_replay)?;
        }

        let ok = txn.commit().await?;
        if !ok {
            return Err(BookmarkMovementError::TransactionFailed);
        }
        Ok(())
    }
}
