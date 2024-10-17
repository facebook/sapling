/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bookmarks::BookmarkKey;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::PushrebaseOntoBookmarkOp;
use bookmarks_movement::Repo;
use bytes::Bytes;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use repo_authorization::AuthorizationContext;

use crate::PushrebaseClient;

pub struct LocalPushrebaseClient<'a, R: Repo> {
    pub ctx: &'a CoreContext,
    pub authz: &'a AuthorizationContext,
    pub repo: &'a R,
    pub hook_manager: &'a HookManager,
}

#[async_trait::async_trait]
impl<'a, R: Repo> PushrebaseClient for LocalPushrebaseClient<'a, R> {
    async fn pushrebase(
        &self,
        bookmark: &BookmarkKey,
        changesets: &[BonsaiChangeset],
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
        log_new_public_commits_to_scribe: bool,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError> {
        let mut op = PushrebaseOntoBookmarkOp::new(bookmark, changesets)
            .with_pushvars(pushvars)
            .with_push_source(cross_repo_push_source)
            .with_bookmark_restrictions(bookmark_restrictions);
        if log_new_public_commits_to_scribe {
            op = op.log_new_public_commits_to_scribe();
        }
        op.run(self.ctx, self.authz, self.repo, self.hook_manager)
            .await
    }
}
