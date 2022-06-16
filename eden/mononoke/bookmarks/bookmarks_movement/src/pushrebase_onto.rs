/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::anyhow;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingArc;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use git_mapping_pushrebase_hook::GitMappingPushrebaseHook;
use globalrev_pushrebase_hook::GlobalrevPushrebaseHook;
use hooks::{CrossRepoPushSource, HookManager};
use metaconfig_types::{
    BookmarkAttrs, InfinitepushParams, PushrebaseParams, SourceControlServiceParams,
};
use mononoke_types::BonsaiChangeset;
use pushrebase_hook::PushrebaseHook;
use pushrebase_mutation_mapping::PushrebaseMutationMappingRef;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_identity::RepoIdentityRef;
use repo_read_write_status::RepoReadWriteFetcher;

use crate::affected_changesets::{AdditionalChangesets, AffectedChangesets};
use crate::repo_lock::{check_repo_lock, RepoLockPushrebaseHook};
use crate::restrictions::{
    check_bookmark_sync_config, BookmarkKindRestrictions, BookmarkMoveAuthorization,
};
use crate::{BookmarkMovementError, Repo};

#[must_use = "PushrebaseOntoBookmarkOp must be run to have an effect"]
pub struct PushrebaseOntoBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    affected_changesets: AffectedChangesets,
    auth: BookmarkMoveAuthorization<'op>,
    bookmark_restrictions: BookmarkKindRestrictions,
    cross_repo_push_source: CrossRepoPushSource,
    pushvars: Option<&'op HashMap<String, Bytes>>,
    hg_replay: Option<&'op pushrebase::HgReplayData>,
}

impl<'op> PushrebaseOntoBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkName,
        changesets: HashSet<BonsaiChangeset>,
    ) -> PushrebaseOntoBookmarkOp<'op> {
        PushrebaseOntoBookmarkOp {
            bookmark,
            affected_changesets: AffectedChangesets::with_source_changesets(changesets),
            auth: BookmarkMoveAuthorization::User,
            bookmark_restrictions: BookmarkKindRestrictions::AnyKind,
            cross_repo_push_source: CrossRepoPushSource::NativeToThisRepo,
            pushvars: None,
            hg_replay: None,
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

    pub fn with_hg_replay_data(mut self, hg_replay: Option<&'op pushrebase::HgReplayData>) -> Self {
        self.hg_replay = hg_replay;
        self
    }

    pub fn with_push_source(mut self, cross_repo_push_source: CrossRepoPushSource) -> Self {
        self.cross_repo_push_source = cross_repo_push_source;
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
    ) -> Result<pushrebase::PushrebaseOutcome, BookmarkMovementError> {
        let kind = self
            .bookmark_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)
            .await?;

        check_bookmark_sync_config(repo, self.bookmark, kind)?;

        if pushrebase_params.block_merges {
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
                BookmarkUpdateReason::Pushrebase,
                kind,
                &self.auth,
                AdditionalChangesets::None,
                self.cross_repo_push_source,
            )
            .await?;

        let mut pushrebase_hooks =
            get_pushrebase_hooks(ctx, repo, &self.bookmark, bookmark_attrs, pushrebase_params)?;

        // For pushrebase, we check the repo lock once at the beginning of the
        // pushrebase operation, and then once more as part of the pushrebase
        // bookmark update transaction, to check if the repo got locked while
        // we were peforming the pushrebase.
        check_repo_lock(
            repo_read_write_fetcher,
            kind,
            self.pushvars,
            repo.repo_permission_checker(),
            ctx.metadata().identities(),
        )
        .await?;

        if let Some(hook) = RepoLockPushrebaseHook::new(
            repo_read_write_fetcher,
            kind,
            self.pushvars,
            repo.repo_permission_checker(),
            ctx.metadata().identities(),
        )
        .await?
        {
            pushrebase_hooks.push(hook);
        }

        let mut flags = pushrebase_params.flags.clone();
        if let Some(rewritedates) = bookmark_attrs.should_rewrite_dates(self.bookmark) {
            // Bookmark config overrides repo flags.rewritedates config
            flags.rewritedates = rewritedates;
        }

        ctx.scuba()
            .clone()
            .add("bookmark", self.bookmark.to_string())
            .log_with_msg("Pushrebase started", None);
        let (stats, result) = pushrebase::do_pushrebase_bonsai(
            ctx,
            repo.as_blob_repo(),
            &flags,
            self.bookmark,
            self.affected_changesets.source_changesets(),
            self.hg_replay,
            pushrebase_hooks.as_slice(),
        )
        .timed()
        .await;

        let mut scuba_logger = ctx.scuba().clone();
        scuba_logger.add_future_stats(&stats);
        match &result {
            Ok(outcome) => scuba_logger
                .add("pushrebase_retry_num", outcome.retry_num.0)
                .add("pushrebase_distance", outcome.pushrebase_distance.0)
                .add("bookmark", self.bookmark.to_string())
                .add("changeset_id", format!("{}", outcome.head))
                .log_with_msg("Pushrebase finished", None),
            Err(err) => scuba_logger.log_with_msg("Pushrebase failed", Some(format!("{:#?}", err))),
        }

        result.map_err(BookmarkMovementError::PushrebaseError)
    }
}

/// Get a Vec of the relevant pushrebase hooks for PushrebaseParams, using this repo when
/// required by those hooks.
pub fn get_pushrebase_hooks(
    ctx: &CoreContext,
    repo: &(
         impl BonsaiGitMappingArc
         + BonsaiGlobalrevMappingArc
         + PushrebaseMutationMappingRef
         + RepoIdentityRef
     ),
    bookmark: &BookmarkName,
    bookmark_attrs: &BookmarkAttrs,
    params: &PushrebaseParams,
) -> Result<Vec<Box<dyn PushrebaseHook>>, BookmarkMovementError> {
    let mut pushrebase_hooks = Vec::new();

    match params.globalrevs_publishing_bookmark.as_ref() {
        Some(globalrevs_publishing_bookmark) if globalrevs_publishing_bookmark == bookmark => {
            let hook = GlobalrevPushrebaseHook::new(
                ctx.clone(),
                repo.bonsai_globalrev_mapping_arc().clone(),
                repo.repo_identity().id(),
            );
            pushrebase_hooks.push(hook);
        }
        Some(globalrevs_publishing_bookmark) => {
            return Err(BookmarkMovementError::PushrebaseInvalidGlobalrevsBookmark {
                bookmark: bookmark.clone(),
                globalrevs_publishing_bookmark: globalrevs_publishing_bookmark.clone(),
            });
        }
        None => {
            // No hook necessary
        }
    };

    for attr in bookmark_attrs.select(bookmark) {
        if let Some(descendant_bookmark) = &attr.params().ensure_ancestor_of {
            return Err(
                BookmarkMovementError::PushrebaseNotAllowedRequiresAncestorsOf {
                    bookmark: bookmark.clone(),
                    descendant_bookmark: descendant_bookmark.clone(),
                },
            );
        }
    }

    if params.populate_git_mapping {
        let hook = GitMappingPushrebaseHook::new(repo.bonsai_git_mapping_arc().clone());
        pushrebase_hooks.push(hook);
    }

    match repo.pushrebase_mutation_mapping().get_hook() {
        Some(hook) => pushrebase_hooks.push(hook),
        None => {}
    }

    Ok(pushrebase_hooks)
}
