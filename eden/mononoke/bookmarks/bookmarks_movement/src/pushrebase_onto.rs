/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::anyhow;
use blobrepo::BlobRepo;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use git_mapping_pushrebase_hook::GitMappingPushrebaseHook;
use globalrev_pushrebase_hook::GlobalrevPushrebaseHook;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushrebaseParams};
use mononoke_types::BonsaiChangeset;
use scuba_ext::ScubaSampleBuilderExt;

use crate::{BookmarkKindRestrictions, BookmarkMoveAuthorization, BookmarkMovementError};

pub struct PushrebaseOntoBookmarkOp<'op> {
    bookmark: &'op BookmarkName,
    changesets: &'op HashSet<BonsaiChangeset>,
    auth: BookmarkMoveAuthorization,
    kind_restrictions: BookmarkKindRestrictions,
    hg_replay: Option<&'op pushrebase::HgReplayData>,
}

#[must_use = "PushrebaseOntoBookmarkOp must be run to have an effect"]
impl<'op> PushrebaseOntoBookmarkOp<'op> {
    pub fn new(
        bookmark: &'op BookmarkName,
        changesets: &'op HashSet<BonsaiChangeset>,
    ) -> PushrebaseOntoBookmarkOp<'op> {
        PushrebaseOntoBookmarkOp {
            bookmark,
            changesets,
            auth: BookmarkMoveAuthorization::Context,
            kind_restrictions: BookmarkKindRestrictions::AnyKind,
            hg_replay: None,
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

    pub fn with_hg_replay_data(mut self, hg_replay: Option<&'op pushrebase::HgReplayData>) -> Self {
        self.hg_replay = hg_replay;
        self
    }

    pub async fn run(
        self,
        ctx: &'op CoreContext,
        repo: &'op BlobRepo,
        infinitepush_params: &'op InfinitepushParams,
        pushrebase_params: &'op PushrebaseParams,
        bookmark_attrs: &'op BookmarkAttrs,
    ) -> Result<pushrebase::PushrebaseOutcome, BookmarkMovementError> {
        self.auth
            .check_authorized(ctx, bookmark_attrs, self.bookmark)?;

        if pushrebase_params.block_merges {
            let any_merges = self.changesets.iter().any(BonsaiChangeset::is_merge);
            if any_merges {
                return Err(anyhow!(
                    "Pushrebase blocked because it contains a merge commit.\n\
                    If you need this for a specific use case please contact\n\
                    the Source Control team at https://fburl.com/27qnuyl2"
                )
                .into());
            }
        }

        self.kind_restrictions
            .check_kind(infinitepush_params, self.bookmark)?;

        let mut pushrebase_hooks = Vec::new();

        if pushrebase_params.assign_globalrevs {
            let hook = GlobalrevPushrebaseHook::new(
                repo.bonsai_globalrev_mapping().clone(),
                repo.get_repoid(),
            );
            pushrebase_hooks.push(hook);
        }

        if pushrebase_params.populate_git_mapping {
            let hook = GitMappingPushrebaseHook::new(repo.bonsai_git_mapping().clone());
            pushrebase_hooks.push(hook);
        }

        let mut flags = pushrebase_params.flags.clone();
        if let Some(rewritedates) = bookmark_attrs.should_rewrite_dates(self.bookmark) {
            // Bookmark config overrides repo flags.rewritedates config
            flags.rewritedates = rewritedates;
        }

        ctx.scuba().clone().log_with_msg("Pushrebase started", None);
        let (stats, result) = pushrebase::do_pushrebase_bonsai(
            ctx,
            repo,
            &flags,
            self.bookmark,
            self.changesets,
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
                .log_with_msg("Pushrebase finished", None),
            Err(err) => scuba_logger.log_with_msg("Pushrebase failed", Some(format!("{:#?}", err))),
        }

        result.map_err(BookmarkMovementError::PushrebaseError)
    }
}
