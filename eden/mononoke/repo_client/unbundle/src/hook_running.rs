/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use crate::{resolver::HookFailure, BundleResolverError, PostResolveAction, PostResolvePushRebase};
use anyhow::Context;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::BoxFuture,
    stream::{FuturesUnordered, TryStreamExt},
    FutureExt, TryFutureExt,
};
use futures_ext::{BoxFuture as OldBoxFuture, FutureExt as _};
use futures_old::future::ok;
use futures_stats::TimedFutureExt;
use hooks::{HookManager, HookOutcome};
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use scuba_ext::ScubaSampleBuilderExt;
use std::{collections::HashMap, sync::Arc};

pub trait RemapAsyncFn = (Fn(ChangesetId) -> BoxFuture<'static, Result<HgChangesetId, BundleResolverError>>)
    + Send
    + Sync
    + 'static;

pub fn run_remapped_hooks(
    ctx: CoreContext,
    hook_manager: Arc<HookManager>,
    action: &PostResolveAction,
    remap_cs: impl RemapAsyncFn,
) -> OldBoxFuture<(), BundleResolverError> {
    match action {
        // TODO: Need to run hooks on Push, not just PushRebase
        PostResolveAction::Push(_) => ok(()).boxify(),
        PostResolveAction::InfinitePush(_) => ok(()).boxify(),
        PostResolveAction::PushRebase(action) => {
            run_pushrebase_hooks(ctx, action, hook_manager, remap_cs)
        }
        PostResolveAction::BookmarkOnlyPushRebase(_) => ok(()).boxify(),
    }
}

pub fn run_hooks(
    ctx: CoreContext,
    repo: BlobRepo,
    hook_manager: Arc<HookManager>,
    action: &PostResolveAction,
) -> OldBoxFuture<(), BundleResolverError> {
    run_remapped_hooks(ctx.clone(), hook_manager, action, move |cs| {
        let repo = repo.clone();
        let ctx = ctx.clone();
        async move {
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs)
                .compat()
                .await
                .map_err(|e| e.into())
        }
        .boxed()
    })
}

fn run_pushrebase_hooks(
    ctx: CoreContext,
    action: &PostResolvePushRebase,
    hook_manager: Arc<HookManager>,
    remap_cs: impl RemapAsyncFn,
) -> OldBoxFuture<(), BundleResolverError> {
    // The changesets that will be pushed
    let changesets = action.uploaded_bonsais.clone();
    let maybe_pushvars = action.maybe_pushvars.clone();
    // FIXME: stop cloning when this fn is async
    let bookmark = action.bookmark_spec.get_bookmark_name().clone();

    async move {
        run_hooks_on_changesets(
            &ctx,
            &*hook_manager,
            changesets.iter(),
            bookmark,
            maybe_pushvars,
            remap_cs,
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
    hook_manager: &HookManager,
    changesets: impl Iterator<Item = &BonsaiChangeset> + Clone + itertools::Itertools,
    bookmark: BookmarkName,
    maybe_pushvars: Option<HashMap<String, Bytes>>,
    remap_cs: impl RemapAsyncFn,
) -> Result<(), BundleResolverError> {
    let (stats, hook_outcomes) = hook_manager
        .run_hooks_for_bookmark(&ctx, changesets, &bookmark, maybe_pushvars.as_ref())
        .timed()
        .await;
    let hook_outcomes = hook_outcomes.context("While running hooks")?;

    let rejections = hook_outcomes
        .into_iter()
        .filter_map(HookOutcome::into_rejection)
        .collect::<Vec<_>>();

    ctx.scuba()
        .clone()
        .add_future_stats(&stats)
        .add("hook_rejections", rejections.len())
        .log_with_msg("Executed hooks", None);

    if rejections.is_empty() {
        return Ok(());
    }

    let remap_cs = &remap_cs;
    let rejections = rejections
        .into_iter()
        .map(|(hook_name, cs_id, info)| async move {
            let cs_id = remap_cs(cs_id).await?;

            Result::<_, anyhow::Error>::Ok(HookFailure {
                hook_name,
                cs_id,
                info,
            })
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect()
        .await?;

    Err(BundleResolverError::HookError(rejections))
}
