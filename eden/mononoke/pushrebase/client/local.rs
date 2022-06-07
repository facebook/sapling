/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::PushrebaseClient;

use bookmarks::BookmarkName;
use bookmarks_movement::{BookmarkMovementError, PushrebaseOntoBookmarkOp, Repo};
use bytes::Bytes;
use context::CoreContext;
use hooks::{CrossRepoPushSource, HookManager};
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushrebaseParams};
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_read_write_status::RepoReadWriteFetcher;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct LocalPushrebaseClient<'a, R: Repo> {
    pub ctx: &'a CoreContext,
    pub repo: &'a R,
    pub pushrebase_params: &'a PushrebaseParams,
    pub lca_hint: &'a Arc<dyn LeastCommonAncestorsHint>,
    pub maybe_hg_replay_data: &'a Option<pushrebase::HgReplayData>,
    pub bookmark_attrs: &'a BookmarkAttrs,
    pub infinitepush_params: &'a InfinitepushParams,
    pub hook_manager: &'a HookManager,
    pub cross_repo_push_source: CrossRepoPushSource,
    pub readonly_fetcher: &'a RepoReadWriteFetcher,
}

#[async_trait::async_trait]
impl<'a, R: Repo> PushrebaseClient for LocalPushrebaseClient<'a, R> {
    async fn pushrebase(
        &self,
        _repo: String,
        bookmark: &BookmarkName,
        changesets: HashSet<BonsaiChangeset>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError> {
        PushrebaseOntoBookmarkOp::new(bookmark, changesets)
            .only_if_public()
            .with_pushvars(pushvars)
            .with_hg_replay_data(self.maybe_hg_replay_data.as_ref())
            .with_push_source(self.cross_repo_push_source)
            .run(
                self.ctx,
                self.repo,
                self.lca_hint,
                self.infinitepush_params,
                self.pushrebase_params,
                self.bookmark_attrs,
                self.hook_manager,
                self.readonly_fetcher,
            )
            .await
    }
}
