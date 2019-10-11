// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bundle2_resolver::{
    BookmarkPush, BundleResolverError, PostResolveAction, PostResolveBookmarkOnlyPushRebase,
    PostResolvePush, PostResolvePushRebase, PushrebaseBookmarkSpec,
};

use bookmarks::{BookmarkUpdateReason, BundleReplayData};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use futures::future::ok;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};

pub fn run_post_resolve_action(
    ctx: CoreContext,
    action: PostResolveAction,
) -> BoxFuture<Bytes, BundleResolverError> {
    match action {
        PostResolveAction::Push(action) => push(ctx, action),
        PostResolveAction::PushRebase(action) => push_rebase(ctx, action),
        PostResolveAction::BookmarkOnlyPushRebase(action) => bookmark_only_push_rebase(ctx, action),
    }
}

fn push(_ctx: CoreContext, action: PostResolvePush) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolvePush {
        resolver,
        changegroup_id,
        bookmark_push,
        maybe_raw_bundle2_id,
        lca_hint,
        allow_non_fast_forward,
    } = action;

    ({
        cloned!(resolver);
        move || {
            let bookmark_ids: Vec<_> = bookmark_push
                .iter()
                .filter_map(|bp| match bp {
                    BookmarkPush::PlainPush(bp) => Some(bp.part_id),
                    BookmarkPush::Infinitepush(..) => None,
                })
                .collect();
            let reason = BookmarkUpdateReason::Push {
                bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
            };
            resolver
                .resolve_bookmark_pushes(bookmark_push, reason, lca_hint, allow_non_fast_forward)
                .map(move |()| (changegroup_id, bookmark_ids))
                .boxify()
        }
    })()
    .context("While updating Bookmarks")
    .from_err()
    .and_then(move |(changegroup_id, bookmark_ids)| {
        resolver.prepare_push_response(changegroup_id, bookmark_ids)
    })
    .context("bundle2_resolver error")
    .from_err()
    .boxify()
}

fn push_rebase(
    ctx: CoreContext,
    action: PostResolvePushRebase,
) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolvePushRebase {
        resolver,
        changesets,
        bookmark_push_part_id,
        bookmark_spec,
        maybe_raw_bundle2_id,
        lca_hint,
        maybe_pushvars,
        commonheads,
        phases_hint,
    } = action;

    let bookmark = bookmark_spec.get_bookmark_name();
    resolver
        .run_hooks(ctx.clone(), changesets.clone(), maybe_pushvars, &bookmark)
        .and_then({
            cloned!(ctx, resolver, lca_hint);
            move |()| {
                match bookmark_spec {
                    PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => resolver
                        .pushrebase(ctx, changesets, &onto_params, maybe_raw_bundle2_id)
                        .left_future(),
                    PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => resolver
                        .force_pushrebase(ctx, lca_hint, plain_push, maybe_raw_bundle2_id)
                        .from_err()
                        .right_future(),
                }
                .map(move |pushrebased_rev| (pushrebased_rev, bookmark, bookmark_push_part_id))
            }
        })
        .and_then({
            cloned!(ctx, resolver);
            move |((pushrebased_rev, pushrebased_changesets), bookmark, bookmark_push_part_id)| {
                // TODO: (dbudischek) T41565649 log pushed changesets as well, not only pushrebased
                let new_commits = pushrebased_changesets.iter().map(|p| p.id_new).collect();

                resolver
                    .log_commits_to_scribe(ctx.clone(), new_commits)
                    .and_then(move |_| {
                        resolver.prepare_pushrebase_response(
                            ctx,
                            commonheads,
                            pushrebased_rev,
                            pushrebased_changesets,
                            bookmark,
                            lca_hint,
                            phases_hint,
                            bookmark_push_part_id,
                        )
                    })
                    .from_err()
            }
        })
        .boxify()
}

fn bookmark_only_push_rebase(
    ctx: CoreContext,
    action: PostResolveBookmarkOnlyPushRebase,
) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolveBookmarkOnlyPushRebase {
        resolver,
        bookmark_push,
        maybe_raw_bundle2_id,
        lca_hint,
        allow_non_fast_forward,
    } = action;

    let part_id = bookmark_push.part_id;
    let pushes = vec![BookmarkPush::PlainPush(bookmark_push)];
    let reason = BookmarkUpdateReason::Pushrebase {
        // Since this a bookmark-only pushrebase, there are no changeset timestamps
        bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
    };
    resolver
        .resolve_bookmark_pushes(pushes, reason, lca_hint, allow_non_fast_forward)
        .and_then(move |_| ok(part_id).boxify())
        .and_then({
            cloned!(resolver, ctx);
            move |bookmark_push_part_id| {
                resolver.prepare_push_bookmark_response(ctx, bookmark_push_part_id, true)
            }
        })
        .from_err()
        .boxify()
}
