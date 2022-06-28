/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::PushrebaseClient;

use bookmarks::BookmarkName;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::PushrebaseOntoBookmarkOp;
use bookmarks_movement::Repo;
use bytes::Bytes;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use metaconfig_types::BookmarkAttrs;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::PushrebaseParams;
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_read_write_status::RepoReadWriteFetcher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

pub struct LocalPushrebaseClient<'a, R: Repo> {
    pub ctx: &'a CoreContext,
    pub repo: &'a R,
    pub pushrebase_params: &'a PushrebaseParams,
    pub lca_hint: &'a Arc<dyn LeastCommonAncestorsHint>,
    pub bookmark_attrs: &'a BookmarkAttrs,
    pub infinitepush_params: &'a InfinitepushParams,
    pub hook_manager: &'a HookManager,
    pub readonly_fetcher: &'a RepoReadWriteFetcher,
}

#[async_trait::async_trait]
impl<'a, R: Repo> PushrebaseClient for LocalPushrebaseClient<'a, R> {
    async fn pushrebase(
        &self,
        bookmark: &BookmarkName,
        changesets: HashSet<BonsaiChangeset>,
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError> {
        PushrebaseOntoBookmarkOp::new(bookmark, changesets)
            .with_pushvars(pushvars)
            .with_push_source(cross_repo_push_source)
            .with_bookmark_restrictions(bookmark_restrictions)
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
