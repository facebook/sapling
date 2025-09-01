/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
use repo_authorization::RepoWriteOperation;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;

use crate::BookmarkInfoData;
use crate::BookmarkInfoTransaction;
use crate::BookmarkMovementError;
use crate::Repo;
use crate::affected_changesets::AdditionalChangesets;
use crate::affected_changesets::AffectedChangesets;
use crate::repo_lock::check_repo_lock;
use crate::restrictions::BookmarkKindRestrictions;
use crate::restrictions::check_bookmark_sync_config;

#[must_use = "CreateBookmarkOp must be run to have an effect"]
pub struct CreateBookmarkOp<'op> {
    bookmark: BookmarkKey,
    target: ChangesetId,
    reason: BookmarkUpdateReason,
    kind_restrictions: BookmarkKindRestrictions,
    cross_repo_push_source: CrossRepoPushSource,
    affected_changesets: AffectedChangesets,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    log_new_public_commits_to_scribe: bool,
    only_log_acl_checks: bool,
}

impl<'op> CreateBookmarkOp<'op> {
    pub fn new(
        bookmark: BookmarkKey,
        target: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> CreateBookmarkOp<'op> {
        CreateBookmarkOp {
            bookmark,
            target,
            reason,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            cross_repo_push_source: CrossRepoPushSource::NativeToThisRepo,
            affected_changesets: AffectedChangesets::new(),
            pushvars: None,
            log_new_public_commits_to_scribe: false,
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

    pub fn log_new_public_commits_to_scribe(mut self) -> Self {
        self.log_new_public_commits_to_scribe = true;
        self
    }

    pub fn only_log_acl_checks(mut self, only_log: bool) -> Self {
        self.only_log_acl_checks = only_log;
        self
    }

    pub fn with_checks_bypassed(mut self) -> Self {
        self.affected_changesets
            .bypass_checks_on_additional_changesets();
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

    pub async fn run_with_transaction(
        mut self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
        hook_manager: &'op HookManager,
        txn: Option<Box<dyn BookmarkTransaction>>,
        mut txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Result<BookmarkInfoTransaction, BookmarkMovementError> {
        let kind = self.kind_restrictions.check_kind(repo, &self.bookmark)?;

        if self.only_log_acl_checks {
            if authz
                .check_repo_write(ctx, repo, RepoWriteOperation::CreateBookmark(kind))
                .await
                .is_denied()
            {
                ctx.scuba()
                    .clone()
                    .log_with_msg("Repo write ACL check would fail for bookmark create", None);
            }
        } else {
            authz
                .require_repo_write(ctx, repo, RepoWriteOperation::CreateBookmark(kind))
                .await?;
        }
        authz
            .require_bookmark_modify(ctx, repo, &self.bookmark)
            .await?;

        check_bookmark_sync_config(ctx, repo, &self.bookmark, kind).await?;

        let validated_changesets = self
            .affected_changesets
            .check_restrictions(
                ctx,
                authz,
                repo,
                hook_manager,
                &self.bookmark,
                self.pushvars,
                self.reason,
                kind,
                AdditionalChangesets::Ancestors(self.target),
                self.cross_repo_push_source,
            )
            .await?;

        check_repo_lock(
            ctx,
            repo,
            kind,
            self.pushvars,
            ctx.metadata().identities(),
            authz,
        )
        .await?;

        let is_mirror_upload = self
            .pushvars
            .and_then(|p| p.get("MIRROR_UPLOAD"))
            .is_some_and(|v| **v == *b"true");

        if is_mirror_upload {
            authz.require_mirror_upload_operations(ctx, repo).await?;
        }

        let mut txn = txn.unwrap_or_else(|| repo.bookmarks().create_transaction(ctx.clone()));

        let commits_to_log = match kind {
            BookmarkKind::Scratch => {
                ctx.scuba()
                    .clone()
                    .add("bookmark", self.bookmark.to_string())
                    .log_with_msg("Creating scratch bookmark", None);

                txn.create_scratch(&self.bookmark, self.target)?;
                vec![]
            }
            BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
                crate::restrictions::check_restriction_ensure_ancestor_of(
                    ctx,
                    repo,
                    &self.bookmark,
                    self.target,
                )
                .await?;

                let txn_hook = crate::git_mapping::populate_git_mapping_txn_hook(
                    ctx,
                    repo,
                    self.target,
                    self.affected_changesets.new_changesets(),
                )
                .await?;
                if let Some(txn_hook) = txn_hook {
                    txn_hooks.push(txn_hook);
                }
                let to_log = if self.log_new_public_commits_to_scribe {
                    validated_changesets
                } else {
                    vec![]
                };
                ctx.scuba()
                    .clone()
                    .add("bookmark", self.bookmark.to_string())
                    .log_with_msg("Creating public bookmark", None);

                if is_mirror_upload {
                    txn.creates_or_updates(&self.bookmark, self.target, self.reason)?;
                } else {
                    txn.create(&self.bookmark, self.target, self.reason)?;
                }

                to_log
            }
        };
        let info = BookmarkInfo {
            bookmark_name: self.bookmark.clone(),
            bookmark_kind: kind,
            operation: BookmarkOperation::Create(self.target),
            reason: self.reason,
        };
        let info_data =
            BookmarkInfoData::new(info, self.log_new_public_commits_to_scribe, commits_to_log);
        Ok(BookmarkInfoTransaction::new(info_data, txn, txn_hooks))
    }

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
        hook_manager: &'op HookManager,
    ) -> Result<BookmarkUpdateLogId, BookmarkMovementError> {
        let info_txn = self
            .run_with_transaction(ctx, authz, repo, hook_manager, None, vec![])
            .await?;
        let log_id = info_txn.commit_and_log(ctx, repo).await?;

        Ok(log_id)
    }
}
