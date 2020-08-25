/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use blobrepo::BlobRepo;
use bookmarks::{BookmarkUpdateReason, BundleReplay};
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use hooks::HookManager;
use metaconfig_types::{
    BookmarkAttrs, InfinitepushParams, PushrebaseParams, SourceControlServiceParams,
};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;

use crate::affected_changesets::{AdditionalChangesets, AffectedChangesets};
use crate::restrictions::{BookmarkKind, BookmarkKindRestrictions, BookmarkMoveAuthorization};
use crate::BookmarkMovementError;

pub struct CreateBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    target: ChangesetId,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization<'op>,
    kind_restrictions: BookmarkKindRestrictions,
    affected_changesets: AffectedChangesets,
    pushvars: Option<&'op HashMap<String, Bytes>>,
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
            auth: BookmarkMoveAuthorization::User,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            affected_changesets: AffectedChangesets::new(),
            pushvars: None,
            bundle_replay: None,
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
        self.kind_restrictions = BookmarkKindRestrictions::OnlyPublic;
        self
    }

    pub fn with_pushvars(mut self, pushvars: Option<&'op HashMap<String, Bytes>>) -> Self {
        self.pushvars = pushvars;
        self
    }

    pub fn with_bundle_replay_data(mut self, bundle_replay: Option<&'op dyn BundleReplay>) -> Self {
        self.bundle_replay = bundle_replay;
        self
    }

    /// Include bonsai changesets for changesets that have just been added to
    /// the repository.
    pub fn with_new_changesets(
        mut self,
        changesets: HashMap<ChangesetId, BonsaiChangeset>,
    ) -> Self {
        self.affected_changesets.add_new_changesets(changesets);
        self
    }

    pub async fn run(
        mut self,
        ctx: &'op CoreContext,
        repo: &'op BlobRepo,
        lca_hint: &'op Arc<dyn LeastCommonAncestorsHint>,
        infinitepush_params: &'op InfinitepushParams,
        pushrebase_params: &'op PushrebaseParams,
        bookmark_attrs: &'op BookmarkAttrs,
        hook_manager: &'op HookManager,
    ) -> Result<(), BookmarkMovementError> {
        let kind = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark, kind)?;

        self.affected_changesets
            .check_restrictions(
                ctx,
                repo,
                lca_hint,
                bookmark_attrs,
                hook_manager,
                self.bookmark,
                self.pushvars,
                self.reason,
                kind,
                &self.auth,
                AdditionalChangesets::Ancestors(self.target),
            )
            .await?;

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        let mut txn_hook = None;

        match kind {
            BookmarkKind::Scratch => {
                txn.create_scratch(self.bookmark, self.target)?;
            }
            BookmarkKind::Public => {
                crate::globalrev_mapping::require_globalrevs_disabled(pushrebase_params)?;
                txn_hook = crate::git_mapping::populate_git_mapping_txn_hook(
                    ctx,
                    repo,
                    pushrebase_params,
                    self.target,
                    self.affected_changesets.new_changesets(),
                )
                .await?;
                txn.create(self.bookmark, self.target, self.reason, self.bundle_replay)?;
            }
        }

        let ok = match txn_hook {
            Some(txn_hook) => txn.commit_with_hook(txn_hook).await?,
            None => txn.commit().await?,
        };
        if !ok {
            return Err(BookmarkMovementError::TransactionFailed);
        }
        Ok(())
    }
}
