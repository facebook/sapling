/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use crate::{BundleResolverError, PostResolveAction, PostResolvePushRebase};
use context::CoreContext;
use futures::{FutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::future::ok;
use hooks::{HookManager, HookOutcome};
use std::sync::Arc;

pub fn run_hooks(
    ctx: CoreContext,
    hook_manager: Arc<HookManager>,
    action: &PostResolveAction,
) -> BoxFuture<(), BundleResolverError> {
    match action {
        // TODO: Need to run hooks on Push, not just PushRebase
        PostResolveAction::Push(_) => ok(()).boxify(),
        PostResolveAction::InfinitePush(_) => ok(()).boxify(),
        PostResolveAction::PushRebase(action) => run_pushrebase_hooks(ctx, action, hook_manager),
        PostResolveAction::BookmarkOnlyPushRebase(_) => ok(()).boxify(),
    }
}

fn run_pushrebase_hooks(
    ctx: CoreContext,
    action: &PostResolvePushRebase,
    hook_manager: Arc<HookManager>,
) -> BoxFuture<(), BundleResolverError> {
    let changesets = action.uploaded_hg_changeset_ids.clone();
    let maybe_pushvars = action.maybe_pushvars.clone();
    let bookmark = action.bookmark_spec.get_bookmark_name();

    async move {
        let hook_failures: Vec<_> = hook_manager
            .run_hooks_for_bookmark(&ctx, changesets, &bookmark, maybe_pushvars.as_ref())
            .await?
            .into_iter()
            .filter(HookOutcome::is_rejection)
            .collect();
        if hook_failures.is_empty() {
            Ok(())
        } else {
            Err(BundleResolverError::HookError(hook_failures))
        }
    }
    .boxed()
    .compat()
    .boxify()
}
