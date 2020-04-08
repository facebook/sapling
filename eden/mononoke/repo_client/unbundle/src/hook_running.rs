/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use crate::{resolver::HookFailure, BundleResolverError, PostResolveAction, PostResolvePushRebase};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::try_join,
    stream::{self, TryStreamExt},
    FutureExt, TryFutureExt,
};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::future::ok;
use hooks::{HookExecution, HookManager, HookOutcome};
use mercurial_types::HgChangesetId;
use mononoke_types::BonsaiChangeset;
use std::{collections::HashMap, sync::Arc};

pub fn run_hooks(
    ctx: CoreContext,
    repo: BlobRepo,
    hook_manager: Arc<HookManager>,
    action: &PostResolveAction,
) -> BoxFuture<(), BundleResolverError> {
    match action {
        // TODO: Need to run hooks on Push, not just PushRebase
        PostResolveAction::Push(_) => ok(()).boxify(),
        PostResolveAction::InfinitePush(_) => ok(()).boxify(),
        PostResolveAction::PushRebase(action) => {
            run_pushrebase_hooks(ctx, repo, action, hook_manager)
        }
        PostResolveAction::BookmarkOnlyPushRebase(_) => ok(()).boxify(),
    }
}

fn run_pushrebase_hooks(
    ctx: CoreContext,
    repo: BlobRepo,
    action: &PostResolvePushRebase,
    hook_manager: Arc<HookManager>,
) -> BoxFuture<(), BundleResolverError> {
    // The changesets that will be pushed
    let changesets = action.uploaded_bonsais.clone();
    let hg = action.uploaded_hg_changeset_ids.clone();
    let maybe_pushvars = action.maybe_pushvars.clone();
    // FIXME: stop cloning when this fn is async
    let bookmark = action.bookmark_spec.get_bookmark_name().clone();

    async move {
        let ((), ()) = try_join(
            run_hooks_on_changesets(
                &ctx,
                &repo,
                &*hook_manager,
                changesets.iter(),
                bookmark.clone(),
                maybe_pushvars.clone(),
            ),
            run_hooks_on_changesets_hg(&ctx, &*hook_manager, hg, bookmark, maybe_pushvars),
        )
        .await?;
        Ok(())
    }
    .boxed()
    .compat()
    .boxify()
}

async fn run_hooks_on_changesets(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hook_manager: &HookManager,
    changesets: impl Iterator<Item = &BonsaiChangeset> + Clone + itertools::Itertools,
    bookmark: BookmarkName,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
) -> Result<(), BundleResolverError> {
    let hook_outcomes = hook_manager
        .run_hooks_for_bookmark_bonsai(&ctx, changesets, &bookmark, maybe_pushvars.as_ref())
        .await?;
    if hook_outcomes.iter().all(HookOutcome::is_accept) {
        Ok(())
    } else {
        let hook_failures = hook_outcomes
            .into_iter()
            .filter_map(|outcome| {
                let hook_name = outcome.get_hook_name().to_string();

                let cs_id = outcome.get_changeset_id();

                let info = match outcome.into() {
                    HookExecution::Accepted => None,
                    HookExecution::Rejected(info) => Some(info),
                }?;

                Some(async move {
                    let cs_id = repo
                        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                        .compat()
                        .await?;

                    Result::<_, anyhow::Error>::Ok(HookFailure {
                        hook_name,
                        cs_id,
                        info,
                    })
                })
            })
            .collect::<futures::stream::FuturesUnordered<_>>()
            .try_collect()
            .await?;
        Err(BundleResolverError::HookError(hook_failures))
    }
}

async fn run_hooks_on_changesets_hg(
    ctx: &CoreContext,
    hook_manager: &HookManager,
    changesets: impl IntoIterator<Item = HgChangesetId>,
    bookmark: BookmarkName,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
) -> Result<(), BundleResolverError> {
    let hook_outcomes = hook_manager
        .run_hooks_for_bookmark(&ctx, changesets, &bookmark, maybe_pushvars.as_ref())
        .await?;
    if hook_outcomes.iter().all(HookOutcome::is_accept) {
        Ok(())
    } else {
        let hook_failures = stream::iter(
            hook_outcomes
                .into_iter()
                .map(|o| -> Result<_, BundleResolverError> { Ok(o) }),
        )
        .try_filter_map(|outcome| async move {
            let hook_name = outcome.get_hook_name().to_string();
            let cs_id = outcome.get_cs_id();
            match outcome.into() {
                HookExecution::Accepted => Ok(None),
                HookExecution::Rejected(info) => Ok(Some(HookFailure {
                    hook_name,
                    cs_id,
                    info,
                })),
            }
        })
        .try_collect()
        .await?;
        Err(BundleResolverError::HookError(hook_failures))
    }
}
