/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use mononoke_types::BonsaiChangeset;
use pushrebase_hooks::get_pushrebase_hooks;
use repo_authorization::AuthorizationContext;
use repo_authorization::RepoWriteOperation;
use repo_update_logger::log_bookmark_operation;
use repo_update_logger::log_new_commits;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;
use repo_update_logger::CommitInfo;

use crate::affected_changesets::AdditionalChangesets;
use crate::affected_changesets::AffectedChangesets;
use crate::repo_lock::check_repo_lock;
use crate::repo_lock::RepoLockPushrebaseHook;
use crate::restrictions::check_bookmark_sync_config;
use crate::restrictions::BookmarkKindRestrictions;
use crate::BookmarkMovementError;
use crate::Repo;

#[must_use = "PushrebaseOntoBookmarkOp must be run to have an effect"]
pub struct PushrebaseOntoBookmarkOp<'op> {
    bookmark: &'op BookmarkKey,
    affected_changesets: AffectedChangesets,
    bookmark_restrictions: BookmarkKindRestrictions,
    cross_repo_push_source: CrossRepoPushSource,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    log_new_public_commits_to_scribe: bool,
    only_log_acl_checks: bool,
}

impl<'op> PushrebaseOntoBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkKey,
        changesets: HashSet<BonsaiChangeset>,
    ) -> PushrebaseOntoBookmarkOp<'op> {
        PushrebaseOntoBookmarkOp {
            bookmark,
            affected_changesets: AffectedChangesets::with_source_changesets(changesets),
            bookmark_restrictions: BookmarkKindRestrictions::AnyKind,
            cross_repo_push_source: CrossRepoPushSource::NativeToThisRepo,
            pushvars: None,
            log_new_public_commits_to_scribe: false,
            only_log_acl_checks: false,
        }
    }

    pub fn only_if_scratch(mut self) -> Self {
        self.bookmark_restrictions = BookmarkKindRestrictions::OnlyScratch;
        self
    }

    pub fn only_if_public(mut self) -> Self {
        self.bookmark_restrictions = BookmarkKindRestrictions::OnlyPublishing;
        self
    }

    pub fn with_bookmark_restrictions(
        mut self,
        bookmark_restrictions: BookmarkKindRestrictions,
    ) -> Self {
        self.bookmark_restrictions = bookmark_restrictions;
        self
    }

    pub fn with_pushvars(mut self, pushvars: Option<&'op HashMap<String, Bytes>>) -> Self {
        self.pushvars = pushvars;
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

    pub fn only_log_acl_checks(mut self, only_log: bool) -> Self {
        self.only_log_acl_checks = only_log;
        self
    }

    pub async fn run(
        mut self,
        ctx: &'op CoreContext,
        authz: &'op AuthorizationContext,
        repo: &'op impl Repo,
        hook_manager: &'op HookManager,
    ) -> Result<pushrebase::PushrebaseOutcome, BookmarkMovementError> {
        let kind = self.bookmark_restrictions.check_kind(repo, self.bookmark)?;

        if self.only_log_acl_checks {
            if authz
                .check_repo_write(ctx, repo, RepoWriteOperation::LandStack(kind))
                .await
                .is_denied()
            {
                ctx.scuba().clone().log_with_msg(
                    "Repo write ACL check would fail for bookmark pushrebase",
                    None,
                );
            }
        } else {
            authz
                .require_repo_write(ctx, repo, RepoWriteOperation::LandStack(kind))
                .await?;
        }
        authz
            .require_bookmark_modify(ctx, repo, self.bookmark)
            .await?;

        check_bookmark_sync_config(repo, self.bookmark, kind)?;

        if repo.repo_config().pushrebase.block_merges {
            let any_merges = self
                .affected_changesets
                .source_changesets()
                .iter()
                .any(BonsaiChangeset::is_merge);
            if any_merges {
                return Err(anyhow!(
                    "Pushrebase blocked because it contains a merge commit.\n\
                    If you need this for a specific use case please contact\n\
                    the Source Control team at https://fburl.com/27qnuyl2"
                )
                .into());
            }
        }
        let reason = BookmarkUpdateReason::Pushrebase;

        self.affected_changesets
            .check_restrictions(
                ctx,
                authz,
                repo,
                hook_manager,
                self.bookmark,
                self.pushvars,
                reason,
                kind,
                AdditionalChangesets::None,
                self.cross_repo_push_source,
            )
            .await?;

        let mut pushrebase_hooks =
            get_pushrebase_hooks(ctx, repo, self.bookmark, &repo.repo_config().pushrebase)?;

        // For pushrebase, we check the repo lock once at the beginning of the
        // pushrebase operation, and then once more as part of the pushrebase
        // bookmark update transaction, to check if the repo got locked while
        // we were peforming the pushrebase.
        check_repo_lock(
            repo,
            kind,
            self.pushvars,
            ctx.metadata().identities(),
            authz,
        )
        .await?;

        if let Some(hook) = RepoLockPushrebaseHook::new(
            repo.repo_identity().id(),
            kind,
            self.pushvars,
            repo.repo_permission_checker(),
            ctx.metadata().identities(),
            authz,
        )
        .await
        {
            pushrebase_hooks.push(hook);
        }

        let mut flags = repo.repo_config().pushrebase.flags.clone();
        if let Some(rewritedates) = repo
            .repo_bookmark_attrs()
            .should_rewrite_dates(self.bookmark)
        {
            // Bookmark config overrides repo flags.rewritedates config
            flags.rewritedates = rewritedates;
        }

        ctx.scuba()
            .clone()
            .add("bookmark", self.bookmark.to_string())
            .log_with_msg("Pushrebase started", None);
        let (stats, result) = pushrebase::do_pushrebase_bonsai(
            ctx,
            repo,
            &flags,
            self.bookmark,
            self.affected_changesets.source_changesets(),
            pushrebase_hooks.as_slice(),
        )
        .timed()
        .await;

        let mut scuba_logger = ctx.scuba().clone();
        scuba_logger.add_future_stats(&stats);
        match &result {
            Ok(outcome) => {
                scuba_logger
                    .add("pushrebase_retry_num", outcome.retry_num.0)
                    .add("pushrebase_distance", outcome.pushrebase_distance.0)
                    .add("bookmark", self.bookmark.to_string())
                    .add("changeset_id", format!("{}", outcome.head))
                    .log_with_msg("Pushrebase finished", None);

                if self.log_new_public_commits_to_scribe {
                    let mut changesets_to_log: HashMap<_, _> = self
                        .affected_changesets
                        .source_changesets()
                        .iter()
                        .map(|bcs| (bcs.get_changeset_id(), CommitInfo::new(bcs, None)))
                        .collect();

                    for pair in outcome.rebased_changesets.iter() {
                        let info = changesets_to_log
                            .get_mut(&pair.id_old)
                            .ok_or_else(|| anyhow!("Missing commit info for {}", pair.id_old))?;
                        info.update_changeset_id(pair.id_old, pair.id_new)?;
                    }

                    log_new_commits(
                        ctx,
                        repo,
                        Some((self.bookmark, kind)),
                        changesets_to_log.into_values().collect(),
                    )
                    .await;
                }

                let info = BookmarkInfo {
                    bookmark_name: self.bookmark.clone(),
                    bookmark_kind: kind,
                    operation: BookmarkOperation::Pushrebase(
                        outcome.old_bookmark_value,
                        outcome.head,
                    ),
                    reason,
                };
                log_bookmark_operation(ctx, repo, &info).await;

                // Marking the pushrebased changeset as public.
                if kind.is_public() {
                    repo.phases()
                        .add_reachable_as_public(ctx, vec![outcome.head.clone()])
                        .await?;
                }
            }
            Err(err) => scuba_logger.log_with_msg("Pushrebase failed", Some(format!("{:#?}", err))),
        }

        result.map_err(BookmarkMovementError::PushrebaseError)
    }
}
