/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::{BookmarkKind, BookmarkName};
use bytes::Bytes;
use context::CoreContext;
use hooks::{CrossRepoPushSource, HookManager};
use metaconfig_types::{
    BookmarkAttrs, InfinitepushParams, PushrebaseParams, SourceControlServiceParams,
};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use repo_read_write_status::RepoReadWriteFetcher;

use crate::affected_changesets::{
    find_draft_ancestors, log_bonsai_commits_to_scribe, AdditionalChangesets, AffectedChangesets,
};
use crate::repo_lock::check_repo_lock;
use crate::restrictions::{
    check_bookmark_sync_config, BookmarkKindRestrictions, BookmarkMoveAuthorization,
};
use crate::{BookmarkMovementError, Repo};

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
        repo: &impl Repo,
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
                .is_ancestor(
                    ctx,
                    &repo.changeset_fetcher_arc().clone(),
                    targets.old,
                    targets.new,
                )
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

#[must_use = "UpdateBookmarkOp must be run to have an effect"]
pub struct UpdateBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    targets: BookmarkUpdateTargets,
    update_policy: BookmarkUpdatePolicy,
    reason: BookmarkUpdateReason,
    auth: BookmarkMoveAuthorization<'op>,
    kind_restrictions: BookmarkKindRestrictions,
    cross_repo_push_source: CrossRepoPushSource,
    affected_changesets: AffectedChangesets,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    log_new_public_commits_to_scribe: bool,
}

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
            auth: BookmarkMoveAuthorization::User,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            cross_repo_push_source: CrossRepoPushSource::NativeToThisRepo,
            affected_changesets: AffectedChangesets::new(),
            pushvars: None,
            log_new_public_commits_to_scribe: false,
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

    /// Include bonsai changesets for changesets that have just been added to
    /// the repository.
    pub fn with_new_changesets(
        mut self,
        changesets: HashMap<ChangesetId, BonsaiChangeset>,
    ) -> Self {
        self.affected_changesets.add_new_changesets(changesets);
        self
    }

    pub fn with_push_source(mut self, cross_repo_push_source: CrossRepoPushSource) -> Self {
        self.cross_repo_push_source = cross_repo_push_source;
        self
    }

    pub fn log_new_public_commits_to_scribe(mut self) -> Self {
        self.log_new_public_commits_to_scribe = true;
        self
    }

    pub async fn run(
        mut self,
        ctx: &'op CoreContext,
        repo: &'op impl Repo,
        lca_hint: &'op Arc<dyn LeastCommonAncestorsHint>,
        infinitepush_params: &'op InfinitepushParams,
        pushrebase_params: &'op PushrebaseParams,
        bookmark_attrs: &'op BookmarkAttrs,
        hook_manager: &'op HookManager,
        repo_read_write_fetcher: &'op RepoReadWriteFetcher,
    ) -> Result<(), BookmarkMovementError> {
        let kind = self
            .kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)
            .await?;

        check_bookmark_sync_config(repo, self.bookmark, kind)?;

        self.update_policy
            .check_update_permitted(
                ctx,
                repo,
                lca_hint.as_ref(),
                bookmark_attrs,
                &self.bookmark,
                &self.targets,
            )
            .await?;

        self.affected_changesets
            .check_restrictions(
                ctx,
                repo,
                lca_hint,
                pushrebase_params,
                bookmark_attrs,
                hook_manager,
                self.bookmark,
                self.pushvars,
                self.reason,
                kind,
                &self.auth,
                AdditionalChangesets::Range {
                    head: self.targets.new,
                    base: self.targets.old,
                },
                self.cross_repo_push_source,
            )
            .await?;

        check_repo_lock(
            repo_read_write_fetcher,
            kind,
            self.pushvars,
            repo.repo_permission_checker(),
            ctx.metadata().identities(),
        )
        .await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let txn_hook;

        let commits_to_log = match kind {
            BookmarkKind::Scratch => {
                txn_hook = None;

                ctx.scuba()
                    .clone()
                    .add("bookmark", self.bookmark.to_string())
                    .log_with_msg("Updating scratch bookmark", None);
                txn.update_scratch(self.bookmark, self.targets.new, self.targets.old)?;

                vec![]
            }
            BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
                crate::restrictions::check_restriction_ensure_ancestor_of(
                    ctx,
                    repo,
                    self.bookmark,
                    bookmark_attrs,
                    pushrebase_params,
                    lca_hint,
                    self.targets.new,
                )
                .await?;

                let txn_hook_fut = crate::git_mapping::populate_git_mapping_txn_hook(
                    ctx,
                    repo,
                    pushrebase_params,
                    self.targets.new,
                    &self.affected_changesets.new_changesets(),
                );

                let to_log = async {
                    if self.log_new_public_commits_to_scribe {
                        let res = find_draft_ancestors(ctx, repo, self.targets.new).await;
                        match res {
                            Ok(bcss) => bcss,
                            Err(err) => {
                                ctx.scuba().clone().log_with_msg(
                                    "Failed to find draft ancestors",
                                    Some(format!("{}", err)),
                                );
                                vec![]
                            }
                        }
                    } else {
                        vec![]
                    }
                };

                let (txn_hook_res, to_log) = futures::join!(txn_hook_fut, to_log);
                txn_hook = txn_hook_res?;

                ctx.scuba()
                    .clone()
                    .add("bookmark", self.bookmark.to_string())
                    .log_with_msg("Updating public bookmark", None);

                txn.update(
                    self.bookmark,
                    self.targets.new,
                    self.targets.old,
                    self.reason,
                )?;
                to_log
            }
        };

        let ok = match txn_hook {
            Some(txn_hook) => txn.commit_with_hook(txn_hook).await?,
            None => txn.commit().await?,
        };
        if !ok {
            return Err(BookmarkMovementError::TransactionFailed);
        }

        if self.log_new_public_commits_to_scribe {
            log_bonsai_commits_to_scribe(
                ctx,
                repo,
                Some(self.bookmark),
                commits_to_log,
                kind,
                infinitepush_params,
                pushrebase_params,
            )
            .await;
        }

        Ok(())
    }
}
