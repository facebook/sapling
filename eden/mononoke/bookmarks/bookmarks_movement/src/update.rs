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
use reachabilityindex::LeastCommonAncestorsHint;

use crate::{BookmarkKindRestrictions, BookmarkMoveAuthorization, BookmarkMovementError};

/// The old and new changeset during a bookmark update.
///
/// This is a struct to make sure it is clear which is the old target and which is the new.
pub struct BookmarkUpdateTargets {
    pub old: ChangesetId,
    pub new: ChangesetId,
}

/// Which kinds of bookmark updates are allowed for a request.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BookmarkUpdatePolicy {
    /// Only allow fast-forward moves (updates where the new target is a descendant
    /// of the old target).
    FastForwardOnly,

    /// Allow any update that is permitted for the bookmark by repo config.
    AnyPermittedByConfig,
}

impl BookmarkUpdatePolicy {
    async fn check_update_permitted(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        lca_hint: &dyn LeastCommonAncestorsHint,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
        targets: &BookmarkUpdateTargets,
    ) -> Result<(), BookmarkMovementError> {
        let fast_forward_only = match self {
            Self::FastForwardOnly => true,
            Self::AnyPermittedByConfig => bookmark_attrs.is_fast_forward_only(&bookmark),
        };
        if fast_forward_only && targets.old != targets.new {
            // Check that this move is a fast-forward move.
            let is_ancestor = lca_hint
                .is_ancestor(ctx, &repo.get_changeset_fetcher(), targets.old, targets.new)
                .await?;
            if !is_ancestor {
                return Err(BookmarkMovementError::NonFastForwardMove {
                    from: targets.old,
                    to: targets.new,
                });
            }
        }
        Ok(())
    }
}

pub struct UpdateBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    targets: BookmarkUpdateTargets,
    update_policy: BookmarkUpdatePolicy,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization,
    kind_restrictions: BookmarkKindRestrictions,
    bundle_replay: Option<&'op dyn BundleReplay>,
}

#[must_use = "UpdateBookmarkOp must be run to have an effect"]
impl<'op> UpdateBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkName,
        targets: BookmarkUpdateTargets,
        update_policy: BookmarkUpdatePolicy,
        reason: BookmarkUpdateReason,
    ) -> UpdateBookmarkOp<'op> {
        UpdateBookmarkOp {
            bookmark,
            targets,
            update_policy,
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
        lca_hint: &'op dyn LeastCommonAncestorsHint,
        infinitepush_params: &'op InfinitepushParams,
        bookmark_attrs: &'op BookmarkAttrs,
    ) -> Result<(), BookmarkMovementError> {
        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)?;

        let is_scratch = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        self.update_policy
            .check_update_permitted(
                ctx,
                repo,
                lca_hint,
                bookmark_attrs,
                &self.bookmark,
                &self.targets,
            )
            .await?;

        let mut txn = repo.update_bookmark_transaction(ctx.clone());

        if is_scratch {
            txn.update_scratch(self.bookmark, self.targets.new, self.targets.old)?;
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
