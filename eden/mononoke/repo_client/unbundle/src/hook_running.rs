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
use futures::{compat::Future01CompatExt, stream::TryStreamExt, FutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::future::ok;
use futures_stats::TimedFutureExt;
use hooks::{HookManager, HookOutcome};
use mononoke_types::BonsaiChangeset;
use scuba_ext::ScubaSampleBuilderExt;
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
    let maybe_pushvars = action.maybe_pushvars.clone();
    // FIXME: stop cloning when this fn is async
    let bookmark = action.bookmark_spec.get_bookmark_name().clone();

    async move {
        run_hooks_on_changesets(
            &ctx,
            &repo,
            &*hook_manager,
            changesets.iter(),
            bookmark,
            maybe_pushvars,
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

    let rejections = rejections
        .into_iter()
        .map(|(hook_name, cs_id, info)| async move {
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
        .collect::<futures::stream::FuturesUnordered<_>>()
        .try_collect()
        .await?;

    Err(BundleResolverError::HookError(rejections))
}
