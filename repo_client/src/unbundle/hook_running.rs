/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use bookmarks::BookmarkName;
use bundle2_resolver::{BundleResolverError, PostResolveAction, PostResolvePushRebase};
use bytes::Bytes;
use context::CoreContext;
use futures::future::ok;
use futures::{stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution, HookManager};
use mercurial_types::HgChangesetId;
use std::collections::HashMap;
use std::sync::Arc;

pub fn run_hooks(
    ctx: CoreContext,
    hook_manager: Arc<HookManager>,
    action: &PostResolveAction,
) -> BoxFuture<(), BundleResolverError> {
    match action {
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
    let changesets: Vec<_> = action.uploaded_hg_bonsai_map.keys().cloned().collect();
    let maybe_pushvars = action.maybe_pushvars.clone();
    let bookmark = action.bookmark_spec.get_bookmark_name();
    run_pushrebase_hooks_impl(ctx, changesets, maybe_pushvars, &bookmark, hook_manager)
}

fn run_pushrebase_hooks_impl(
    ctx: CoreContext,
    changesets: Vec<HgChangesetId>,
    pushvars: Option<HashMap<String, Bytes>>,
    onto_bookmark: &BookmarkName,
    hook_manager: Arc<HookManager>,
) -> BoxFuture<(), BundleResolverError> {
    // TODO: should we also accept the Option<HgBookmarkPush> and run hooks on that?
    let mut futs = stream::FuturesUnordered::new();
    for hg_cs_id in changesets {
        futs.push(
            hook_manager
                .run_changeset_hooks_for_bookmark(
                    ctx.clone(),
                    hg_cs_id.clone(),
                    onto_bookmark,
                    pushvars.clone(),
                )
                .join(hook_manager.run_file_hooks_for_bookmark(
                    ctx.clone(),
                    hg_cs_id,
                    onto_bookmark,
                    pushvars.clone(),
                )),
        )
    }
    futs.collect()
        .from_err()
        .and_then(|res| {
            let (cs_hook_results, file_hook_results): (Vec<_>, Vec<_>) = res.into_iter().unzip();
            let cs_hook_failures: Vec<(ChangesetHookExecutionID, HookExecution)> = cs_hook_results
                .into_iter()
                .flatten()
                .filter(|(_, exec)| match exec {
                    HookExecution::Accepted => false,
                    HookExecution::Rejected(_) => true,
                })
                .collect();
            let file_hook_failures: Vec<(FileHookExecutionID, HookExecution)> = file_hook_results
                .into_iter()
                .flatten()
                .filter(|(_, exec)| match exec {
                    HookExecution::Accepted => false,
                    HookExecution::Rejected(_) => true,
                })
                .collect();
            if cs_hook_failures.len() > 0 || file_hook_failures.len() > 0 {
                Err(BundleResolverError::HookError((
                    cs_hook_failures,
                    file_hook_failures,
                )))
            } else {
                Ok(())
            }
        })
        .boxify()
}
