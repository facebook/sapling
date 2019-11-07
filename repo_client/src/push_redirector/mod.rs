/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use crate::mononoke_repo::MononokeRepo;
use crate::unbundle::response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};
use crate::unbundle::run_post_resolve_action;

use backsyncer::TargetRepoDbs;
use bundle2_resolver::{
    BundleResolverError, PostResolveAction, PostResolveBookmarkOnlyPushRebase,
    PostResolveInfinitePush, PostResolvePush, PostResolvePushRebase,
};
use context::CoreContext;
use cross_repo_sync::CommitSyncer;
use failure::Error;
use futures::Future;
use futures_preview::compat::Future01CompatExt;
use futures_util::{future::FutureExt, try_future::TryFutureExt};
use std::sync::Arc;
use synced_commit_mapping::SyncedCommitMapping;

#[derive(Clone)]
pub struct RepoSyncTarget {
    // target (large) repo to sync into
    pub repo: MononokeRepo,
    // `CommitSyncer` struct to do push redirecion
    pub small_to_large_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // `CommitSyncer` struct for the backsyncer
    pub large_to_small_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // A struct, needed to backsync commits
    pub target_repo_dbs: TargetRepoDbs,
}

impl RepoSyncTarget {
    /// To the external observer, this fn is just like `run_post_resolve_action`
    /// in that it will result in the repo having the action processed.
    /// Under the hood it will:
    /// - convert small repo `PostResolveAction` into a large repo `PostResolveAction`
    /// - run the result of this conversion against the large repo
    /// - trigger a commit backsyncing into the small repo
    /// - convert the `UnbundleResponse` struct to be a small-repo one
    pub fn run_redirected_post_resolve_action_compat(
        self,
        ctx: CoreContext,
        action: PostResolveAction,
    ) -> impl Future<Item = UnbundleResponse, Error = BundleResolverError> {
        async move { self.run_redirected_post_resolve_action(ctx, action).await }
            .boxed()
            .compat()
    }

    /// To the external observer, this fn is just like `run_post_resolve_action`
    /// in that it will result in the repo having the action processed.
    /// Under the hood it will:
    /// - convert small repo `PostResolveAction` into a large repo `PostResolveAction`
    /// - run the result of this conversion against the large repo
    /// - trigger a commit backsyncing into the small repo
    /// - convert the `UnbundleResponse` struct to be a small-repo one
    pub async fn run_redirected_post_resolve_action(
        &self,
        ctx: CoreContext,
        action: PostResolveAction,
    ) -> Result<UnbundleResponse, BundleResolverError> {
        let large_repo = self.repo.blobrepo().clone();
        let bookmark_attrs = self.repo.bookmark_attrs();
        let lca_hint = self.repo.lca_hint();
        let phases = self.repo.phases_hint();
        let infinitepush_params = self.repo.infinitepush().clone();
        let puhsrebase_params = self.repo.pushrebase_params().clone();

        let large_repo_action = self
            .convert_post_resolve_action(ctx.clone(), action)
            .await
            .map_err(BundleResolverError::from)?;
        let large_repo_response = run_post_resolve_action(
            ctx.clone(),
            large_repo,
            bookmark_attrs,
            lca_hint,
            phases,
            infinitepush_params,
            puhsrebase_params,
            large_repo_action,
        )
        .compat()
        .map_err(BundleResolverError::from)
        .await?;
        self.convert_unbundle_response(ctx.clone(), large_repo_response)
            .await
            .map_err(BundleResolverError::from)
    }

    /// Convert `PostResolveAction` enum in a small-to-large direction
    /// to be suitable for processing in the large repo
    async fn convert_post_resolve_action(
        &self,
        ctx: CoreContext,
        orig: PostResolveAction,
    ) -> Result<PostResolveAction, Error> {
        use PostResolveAction::*;
        match orig {
            Push(action) => self
                .convert_post_resolve_push_action(ctx, action)
                .await
                .map(Push),
            PushRebase(action) => self
                .convert_post_resolve_pushrebase_action(ctx, action)
                .await
                .map(PushRebase),
            InfinitePush(action) => self
                .convert_post_resolve_infinitepush_action(ctx, action)
                .await
                .map(InfinitePush),
            BookmarkOnlyPushRebase(action) => self
                .convert_post_resolve_bookmark_only_pushrebase_action(ctx, action)
                .await
                .map(BookmarkOnlyPushRebase),
        }
    }

    /// Convert `PostResolvePush` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_push_action(
        &self,
        _ctx: CoreContext,
        _orig: PostResolvePush,
    ) -> Result<PostResolvePush, Error> {
        unimplemented!("convert_post_resolve_push_action")
    }

    /// Convert `PostResolvePushRebase` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_pushrebase_action(
        &self,
        _ctx: CoreContext,
        _orig: PostResolvePushRebase,
    ) -> Result<PostResolvePushRebase, Error> {
        unimplemented!("convert_post_resolve_pushrebase_action")
    }

    /// Convert `PostResolveInfinitePush` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_infinitepush_action(
        &self,
        _ctx: CoreContext,
        _orig: PostResolveInfinitePush,
    ) -> Result<PostResolveInfinitePush, Error> {
        unimplemented!("convert_post_resolve_infinitepush_action")
    }

    /// Convert a `PostResolveBookmarkOnlyPushRebase` in a small-to-large
    /// direction, to be suitable for a processing in a large repo
    async fn convert_post_resolve_bookmark_only_pushrebase_action(
        &self,
        _ctx: CoreContext,
        _orig: PostResolveBookmarkOnlyPushRebase,
    ) -> Result<PostResolveBookmarkOnlyPushRebase, Error> {
        unimplemented!("convert_post_resolve_bookmark_only_pushrebase_action")
    }

    /// Convert `UnbundleResponse` enum in a large-to-small direction
    /// to be suitable for response generation in the small repo
    async fn convert_unbundle_response(
        &self,
        ctx: CoreContext,
        orig: UnbundleResponse,
    ) -> Result<UnbundleResponse, Error> {
        use UnbundleResponse::*;
        match orig {
            PushRebase(resp) => Ok(PushRebase(
                self.convert_unbundle_pushrebase_response(ctx, resp).await?,
            )),
            BookmarkOnlyPushRebase(resp) => Ok(BookmarkOnlyPushRebase(
                self.convert_unbundle_bookmark_only_pushrebase_response(ctx, resp)
                    .await?,
            )),
            Push(resp) => Ok(Push(self.convert_unbundle_push_response(ctx, resp).await?)),
            InfinitePush(resp) => Ok(InfinitePush(
                self.convert_unbundle_infinite_push_response(ctx, resp)
                    .await?,
            )),
        }
    }

    /// Convert `UnbundlePushRebaseResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_pushrebase_response(
        &self,
        _ctx: CoreContext,
        _orig: UnbundlePushRebaseResponse,
    ) -> Result<UnbundlePushRebaseResponse, Error> {
        unimplemented!("convert_unbundle_pushrebase_response")
    }

    /// Convert `UnbundleBookmarkOnlyPushRebaseResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_bookmark_only_pushrebase_response(
        &self,
        _ctx: CoreContext,
        _orig: UnbundleBookmarkOnlyPushRebaseResponse,
    ) -> Result<UnbundleBookmarkOnlyPushRebaseResponse, Error> {
        unimplemented!("convert_unbundle_bookmark_only_pushrebase_response")
    }

    /// Convert `UnbundlePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_push_response(
        &self,
        _ctx: CoreContext,
        _orig: UnbundlePushResponse,
    ) -> Result<UnbundlePushResponse, Error> {
        unimplemented!("convert_unbundle_push_response")
    }

    /// Convert `UnbundleInfinitePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_infinite_push_response(
        &self,
        _ctx: CoreContext,
        _orig: UnbundleInfinitePushResponse,
    ) -> Result<UnbundleInfinitePushResponse, Error> {
        unimplemented!("convert_unbundle_infinite_push_response")
    }
}
