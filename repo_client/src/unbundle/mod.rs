// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use crate::getbundle_response;

use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, BookmarkUpdateReason, BundleReplayData, Transaction};
use bundle2_resolver::{
    BookmarkPush, BundleResolverError, Changesets, CommonHeads, InfiniteBookmarkPush,
    PlainBookmarkPush, PostResolveAction, PostResolveBookmarkOnlyPushRebase, PostResolvePush,
    PostResolvePushRebase, PushrebaseBookmarkSpec,
};
use bytes::{Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, format_err, Error};
use failure_ext::FutureFailureErrorExt;
pub use failure_ext::{prelude::*, Fail};
use futures::future::{err, ok};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_stats::Timed;
use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution, HookManager};
use mercurial_bundles::{create_bundle_stream, parts, Bundle2EncodeBuilder, PartId};
use mercurial_types::HgChangesetId;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushrebaseParams};
use mononoke_types::{ChangesetId, RawBundle2Id};
use obsolete;
use phases::Phases;
use pushrebase;
use reachabilityindex::LeastCommonAncestorsHint;
use scribe_commit_queue::{self, ScribeCommitQueue};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{o, warn};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

pub fn run_post_resolve_action(
    ctx: CoreContext,
    repo: BlobRepo,
    hook_manager: Arc<HookManager>,
    bookmark_attrs: BookmarkAttrs,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    infinitepush_params: InfinitepushParams,
    pushrebase_params: PushrebaseParams,
    action: PostResolveAction,
) -> BoxFuture<Bytes, BundleResolverError> {
    match action {
        PostResolveAction::Push(action) => run_push(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            action,
        ),
        PostResolveAction::PushRebase(action) => run_pushrebase(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            phases_hint,
            hook_manager,
            infinitepush_params,
            pushrebase_params,
            action,
        ),
        PostResolveAction::BookmarkOnlyPushRebase(action) => run_bookmark_only_pushrebase(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            action,
        ),
    }
}

fn run_push(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    infinitepush_params: InfinitepushParams,
    action: PostResolvePush,
) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolvePush {
        changegroup_id,
        bookmark_push,
        maybe_raw_bundle2_id,
        allow_non_fast_forward,
    } = action;

    ({
        cloned!(ctx);
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
            resolve_bookmark_pushes(
                ctx,
                repo,
                bookmark_push,
                reason,
                lca_hint,
                allow_non_fast_forward,
                bookmark_attrs,
                infinitepush_params,
            )
            .map(move |()| (changegroup_id, bookmark_ids))
            .boxify()
        }
    })()
    .context("While updating Bookmarks")
    .from_err()
    .and_then(move |(changegroup_id, bookmark_ids)| {
        prepare_push_response(changegroup_id, bookmark_ids)
    })
    .context("bundle2_resolver error")
    .from_err()
    .boxify()
}

fn run_pushrebase(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    hook_manager: Arc<HookManager>,
    infinitepush_params: InfinitepushParams,
    pushrebase_params: PushrebaseParams,
    action: PostResolvePushRebase,
) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolvePushRebase {
        changesets,
        bookmark_push_part_id,
        bookmark_spec,
        maybe_raw_bundle2_id,
        maybe_pushvars,
        commonheads,
    } = action;

    let bookmark = bookmark_spec.get_bookmark_name();
    run_hooks(
        ctx.clone(),
        changesets.clone(),
        maybe_pushvars,
        &bookmark,
        hook_manager,
    )
    .and_then({
        cloned!(ctx, lca_hint, repo, pushrebase_params);
        move |()| {
            match bookmark_spec {
                PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => normal_pushrebase(
                    ctx,
                    repo.clone(),
                    pushrebase_params,
                    changesets,
                    &onto_params,
                    maybe_raw_bundle2_id,
                    bookmark_attrs,
                    infinitepush_params,
                )
                .left_future(),
                PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => force_pushrebase(
                    ctx,
                    repo,
                    lca_hint,
                    plain_push,
                    maybe_raw_bundle2_id,
                    bookmark_attrs,
                    infinitepush_params,
                )
                .from_err()
                .right_future(),
            }
            .map(move |pushrebased_rev| (pushrebased_rev, bookmark, bookmark_push_part_id))
        }
    })
    .and_then({
        cloned!(ctx, repo);
        move |((pushrebased_rev, pushrebased_changesets), bookmark, bookmark_push_part_id)| {
            // TODO: (dbudischek) T41565649 log pushed changesets as well, not only pushrebased
            let new_commits = pushrebased_changesets.iter().map(|p| p.id_new).collect();

            log_commits_to_scribe(
                ctx.clone(),
                repo.clone(),
                new_commits,
                pushrebase_params.commit_scribe_category.clone(),
            )
            .and_then(move |_| {
                prepare_pushrebase_response(
                    ctx,
                    repo,
                    commonheads,
                    pushrebase_params,
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

fn run_bookmark_only_pushrebase(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    infinitepush_params: InfinitepushParams,
    action: PostResolveBookmarkOnlyPushRebase,
) -> BoxFuture<Bytes, BundleResolverError> {
    let PostResolveBookmarkOnlyPushRebase {
        bookmark_push,
        maybe_raw_bundle2_id,
        allow_non_fast_forward,
    } = action;

    let part_id = bookmark_push.part_id;
    let pushes = vec![BookmarkPush::PlainPush(bookmark_push)];
    let reason = BookmarkUpdateReason::Pushrebase {
        // Since this a bookmark-only pushrebase, there are no changeset timestamps
        bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
    };
    resolve_bookmark_pushes(
        ctx.clone(),
        repo,
        pushes,
        reason,
        lca_hint,
        allow_non_fast_forward,
        bookmark_attrs,
        infinitepush_params,
    )
    .and_then(move |_| ok(part_id).boxify())
    .and_then({
        cloned!(ctx);
        move |bookmark_push_part_id| {
            prepare_push_bookmark_response(ctx, bookmark_push_part_id, true)
        }
    })
    .from_err()
    .boxify()
}

fn prepare_push_bookmark_response(
    _ctx: CoreContext,
    bookmark_push_part_id: PartId,
    success: bool,
) -> impl Future<Item = Bytes, Error = Error> {
    let writer = Cursor::new(Vec::new());
    let mut bundle = Bundle2EncodeBuilder::new(writer);
    // Mercurial currently hangs while trying to read compressed bundles over the wire:
    // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
    // TODO: possibly enable compression support once this is fixed.
    bundle.set_compressor_type(None);
    bundle.add_part(try_boxfuture!(parts::replypushkey_part(
        success,
        bookmark_push_part_id
    )));
    bundle
        .build()
        .map(|cursor| Bytes::from(cursor.into_inner()))
        .context("While preparing response")
        .from_err()
        .boxify()
}

fn prepare_pushrebase_response(
    ctx: CoreContext,
    repo: BlobRepo,
    commonheads: CommonHeads,
    pushrebase_params: PushrebaseParams,
    pushrebased_rev: ChangesetId,
    pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
    onto: BookmarkName,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases: Arc<dyn Phases>,
    bookmark_push_part_id: Option<PartId>,
) -> impl Future<Item = Bytes, Error = Error> {
    // Send to the client both pushrebased commit and current "onto" bookmark. Normally they
    // should be the same, however they might be different if bookmark
    // suddenly moved before current pushrebase finished.
    let common = commonheads.heads;
    let maybe_onto_head = repo.get_bookmark(ctx.clone(), &onto);

    // write phase as public for this commit
    let pushrebased_rev = phases
        .add_reachable_as_public(ctx.clone(), repo.clone(), vec![pushrebased_rev.clone()])
        .and_then({
            cloned!(ctx, repo);
            move |_| repo.get_hg_from_bonsai_changeset(ctx, pushrebased_rev)
        });

    let bookmark_reply_part = match bookmark_push_part_id {
        Some(part_id) => Some(try_boxfuture!(parts::replypushkey_part(true, part_id))),
        None => None,
    };

    let obsmarkers_part = match pushrebase_params.emit_obsmarkers {
        true => try_boxfuture!(obsolete::pushrebased_changesets_to_obsmarkers_part(
            ctx.clone(),
            &repo,
            pushrebased_changesets,
        )
        .transpose()),
        false => None,
    };

    let mut scuba_logger = ctx.scuba().clone();
    maybe_onto_head
        .join(pushrebased_rev)
        .and_then(move |(maybe_onto_head, pushrebased_rev)| {
            let mut heads = vec![];
            if let Some(onto_head) = maybe_onto_head {
                heads.push(onto_head);
            }
            heads.push(pushrebased_rev);
            getbundle_response::create_getbundle_response(
                ctx,
                repo,
                common,
                heads,
                lca_hint,
                Some(phases),
            )
        })
        .and_then(move |mut cg_part_builder| {
            cg_part_builder.extend(bookmark_reply_part.into_iter());
            cg_part_builder.extend(obsmarkers_part.into_iter());
            let compression = None;
            create_bundle_stream(cg_part_builder, compression)
                .collect()
                .map(|chunks| {
                    let mut total_capacity = 0;
                    for c in chunks.iter() {
                        total_capacity += c.len();
                    }

                    // TODO(stash): make push and pushrebase response streamable - T34090105
                    let mut res = BytesMut::with_capacity(total_capacity);
                    for c in chunks {
                        res.extend_from_slice(&c);
                    }
                    res.freeze()
                })
                .context("While preparing response")
                .from_err()
        })
        .timed({
            move |stats, result| {
                if result.is_ok() {
                    scuba_logger
                        .add_future_stats(&stats)
                        .log_with_msg("Pushrebase: prepared the response", None);
                }
                Ok(())
            }
        })
}

/// Takes a changegroup id and prepares a Bytes response containing Bundle2 with reply to
/// changegroup part saying that the push was successful
fn prepare_push_response(
    changegroup_id: Option<PartId>,
    bookmark_ids: Vec<PartId>,
) -> BoxFuture<Bytes, Error> {
    let writer = Cursor::new(Vec::new());
    let mut bundle = Bundle2EncodeBuilder::new(writer);
    // Mercurial currently hangs while trying to read compressed bundles over the wire:
    // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
    // TODO: possibly enable compression support once this is fixed.
    bundle.set_compressor_type(None);
    if let Some(changegroup_id) = changegroup_id {
        bundle.add_part(try_boxfuture!(parts::replychangegroup_part(
            parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
            changegroup_id,
        )));
    }
    for part_id in bookmark_ids {
        bundle.add_part(try_boxfuture!(parts::replypushkey_part(true, part_id)));
    }
    bundle
        .build()
        .map(|cursor| Bytes::from(cursor.into_inner()))
        .context("While preparing response")
        .from_err()
        .boxify()
}

fn normal_pushrebase(
    ctx: CoreContext,
    repo: BlobRepo,
    mut pushrebase_params: PushrebaseParams,
    changesets: Changesets,
    onto_bookmark: &pushrebase::OntoBookmarkParams,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
    bookmark_attrs: BookmarkAttrs,
    infinitepush_params: InfinitepushParams,
) -> impl Future<
    Item = (ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>),
    Error = BundleResolverError,
> {
    let bookmark = &onto_bookmark.bookmark;
    let pushrebase = {
        if let Some(rewritedates) = bookmark_attrs.should_rewrite_dates(bookmark) {
            // Bookmark config overrides repo pushrebase.rewritedates config
            pushrebase_params.rewritedates = rewritedates;
        }
        pushrebase_params
    };

    if let Err(error) = check_plain_bookmark_move_preconditions(
        &ctx,
        &bookmark,
        "pushrebase",
        &bookmark_attrs,
        &infinitepush_params,
    ) {
        return err(error).from_err().boxify();
    }

    let block_merges = pushrebase.block_merges.clone();
    if block_merges
        && changesets
            .iter()
            .any(|(_, revlog_cs)| revlog_cs.p1.is_some() && revlog_cs.p2.is_some())
    {
        return err(format_err!(
            "Pushrebase blocked because it contains a merge commit.\n\
             If you need this for a specific use case please contact\n\
             the Source Control team at https://fburl.com/27qnuyl2"
        ))
        .from_err()
        .boxify();
    }

    futures::lazy({
        cloned!(repo, pushrebase, onto_bookmark, ctx);
        move || {
            ctx.scuba().clone().log_with_msg("pushrebase started", None);
            pushrebase::do_pushrebase(
                ctx,
                repo,
                pushrebase,
                onto_bookmark,
                changesets
                    .into_iter()
                    .map(|(hg_cs_id, _)| hg_cs_id)
                    .collect(),
                maybe_raw_bundle2_id,
            )
        }
    })
    .map_err(|err| match err {
        pushrebase::PushrebaseError::Conflicts(conflicts) => {
            BundleResolverError::PushrebaseConflicts(conflicts)
        }
        _ => BundleResolverError::Error(err_msg(format!("pushrebase failed {:?}", err))),
    })
    .timed({
        let mut scuba_logger = ctx.scuba().clone();
        move |stats, result| {
            if let Ok(res) = result {
                scuba_logger
                    .add_future_stats(&stats)
                    .add("pushrebase_retry_num", res.retry_num)
                    .log_with_msg("Pushrebase finished", None);
            }
            Ok(())
        }
    })
    .map(|res| (res.head, res.rebased_changesets))
    .boxify()
}

fn force_pushrebase(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    bookmark_push: PlainBookmarkPush<HgChangesetId>,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
    bookmark_attrs: BookmarkAttrs,
    infinitepush_params: InfinitepushParams,
) -> impl Future<Item = (ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), Error = Error> {
    bonsai_from_hg_opt(ctx.clone(), &repo.clone(), bookmark_push.new.clone()).and_then(
        move |maybe_target_bcs| {
            let target_bcs =
                try_boxfuture!(maybe_target_bcs
                    .ok_or(err_msg("new changeset is required for force pushrebase")));
            let pushes = vec![BookmarkPush::PlainPush(bookmark_push)];
            let reason = BookmarkUpdateReason::Pushrebase {
                bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
            };
            // Note that this push did not do any actual rebases, so we do not
            // need to provide any actual mapping, an empty Vec will do
            let ret = (target_bcs, Vec::new());
            resolve_bookmark_pushes(
                ctx,
                repo,
                pushes,
                reason,
                lca_hint,
                true,
                bookmark_attrs,
                infinitepush_params,
            )
            .map(move |_| ret)
            .boxify()
        },
    )
}

/// Produce a future that creates a transaction with potentitally multiple bookmark pushes
fn resolve_bookmark_pushes(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_pushes: Vec<BookmarkPush<HgChangesetId>>,
    reason: BookmarkUpdateReason,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    allow_non_fast_forward: bool,
    bookmark_attrs: BookmarkAttrs,
    infinitepush_params: InfinitepushParams,
) -> impl Future<Item = (), Error = Error> {
    let bookmarks_push_fut = bookmark_pushes
        .into_iter()
        .map({
            cloned!(ctx, repo);
            move |bp| {
                hg_bookmark_push_to_bonsai(ctx.clone(), &repo.clone(), bp).and_then({
                    cloned!(repo, ctx, lca_hint, bookmark_attrs, infinitepush_params);
                    move |bp| match bp {
                        BookmarkPush::PlainPush(bp) => check_bookmark_push_allowed(
                            ctx.clone(),
                            repo.clone(),
                            bookmark_attrs,
                            allow_non_fast_forward,
                            infinitepush_params,
                            bp,
                            lca_hint,
                        )
                        .map(|bp| Some(BookmarkPush::PlainPush(bp)))
                        .left_future(),
                        BookmarkPush::Infinitepush(bp) => filter_or_check_infinitepush_allowed(
                            ctx.clone(),
                            repo.clone(),
                            lca_hint,
                            infinitepush_params,
                            bp,
                        )
                        .map(|maybe_bp| maybe_bp.map(BookmarkPush::Infinitepush))
                        .right_future(),
                    }
                })
            }
        })
        .collect::<Vec<_>>();

    future::join_all(bookmarks_push_fut).and_then({
        cloned!(ctx, repo);
        move |bonsai_bookmark_pushes| {
            if bonsai_bookmark_pushes.is_empty() {
                // If we have no bookmarks, then don't create an empty transaction. This is a
                // temporary workaround for the fact that we committing an empty transaction
                // evicts the cache.
                return ok(()).boxify();
            }

            let mut txn = repo.update_bookmark_transaction(ctx.clone());

            for bp in bonsai_bookmark_pushes.into_iter().flatten() {
                try_boxfuture!(add_bookmark_to_transaction(&mut txn, bp, reason.clone()));
            }

            txn.commit()
                .and_then(|ok| {
                    if ok {
                        Ok(())
                    } else {
                        Err(format_err!("Bookmark transaction failed"))
                    }
                })
                .boxify()
        }
    })
}

fn check_bookmark_push_allowed(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark_attrs: BookmarkAttrs,
    allow_non_fast_forward: bool,
    infinitepush_params: InfinitepushParams,
    bp: PlainBookmarkPush<ChangesetId>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
) -> impl Future<Item = PlainBookmarkPush<ChangesetId>, Error = Error> {
    if let Err(error) = check_plain_bookmark_move_preconditions(
        &ctx,
        &bp.name,
        "push",
        &bookmark_attrs,
        &infinitepush_params,
    ) {
        return err(error).right_future();
    }

    let fastforward_only_bookmark = bookmark_attrs.is_fast_forward_only(&bp.name);
    // only allow non fast forward moves if the pushvar is set and the bookmark does not
    // explicitly block them.
    let block_non_fast_forward = fastforward_only_bookmark || !allow_non_fast_forward;

    match (bp.old, bp.new) {
        (old, Some(new)) if block_non_fast_forward => {
            check_is_ancestor_opt(ctx, repo, lca_hint, old, new)
                .map(|_| bp)
                .left_future()
        }
        (Some(_old), None) if fastforward_only_bookmark => Err(format_err!(
            "Deletion of bookmark {} is forbidden.",
            bp.name
        ))
        .into_future()
        .right_future(),
        _ => Ok(bp).into_future().right_future(),
    }
}

fn filter_or_check_infinitepush_allowed(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    infinitepush_params: InfinitepushParams,
    bp: InfiniteBookmarkPush<ChangesetId>,
) -> impl Future<Item = Option<InfiniteBookmarkPush<ChangesetId>>, Error = Error> {
    match infinitepush_params.namespace {
        Some(namespace) => ok(bp)
            // First, check that we match the namespace.
            .and_then(move |bp| match namespace.matches_bookmark(&bp.name) {
                true => ok(bp),
                false => err(format_err!(
                    "Invalid Infinitepush bookmark: {} (Infinitepush bookmarks must match pattern {})",
                    &bp.name,
                    namespace.as_str()
                ))
            })
            // Now, check that the bookmark we want to update either exists or is being created.
            .and_then(|bp| {
                if bp.old.is_some() || bp.create {
                    Ok(bp)
                } else {
                    let e = format_err!(
                        "Unknown bookmark: {}. Use --create to create one.",
                        bp.name
                    );
                    Err(e)
                }
            })
            // Finally,, check that the push is a fast-forward, or --force is set.
            .and_then(|bp| match bp.force {
                true => ok(()).left_future(),
                false => check_is_ancestor_opt(ctx, repo, lca_hint, bp.old, bp.new)
                    .map_err(|e| format_err!("{} (try --force?)", e))
                    .right_future(),
            }.map(|_| bp))
            .map(Some)
            .left_future(),
        None => {
            warn!(ctx.logger(), "Infinitepush bookmark push to {} was ignored", bp.name; o!("remote" => "true"));
            ok(None)
        }.right_future()
    }
    .context("While verifying Infinite Push bookmark push")
    .from_err()
}

fn check_plain_bookmark_move_preconditions(
    ctx: &CoreContext,
    bookmark: &BookmarkName,
    reason: &'static str,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<()> {
    let user = ctx.user_unix_name();
    if !bookmark_attrs.is_allowed_user(user, bookmark) {
        return Err(format_err!(
            "[{}] This user `{:?}` is not allowed to move `{:?}`",
            reason,
            user,
            bookmark
        ));
    }

    if let Some(ref namespace) = infinitepush_params.namespace {
        if namespace.matches_bookmark(bookmark) {
            return Err(format_err!(
                "[{}] Only Infinitepush bookmarks are allowed to match pattern {}",
                reason,
                namespace.as_str(),
            ));
        }
    }

    Ok(())
}

fn add_bookmark_to_transaction(
    txn: &mut Box<dyn Transaction>,
    bookmark_push: BookmarkPush<ChangesetId>,
    reason: BookmarkUpdateReason,
) -> Result<()> {
    match bookmark_push {
        BookmarkPush::PlainPush(PlainBookmarkPush { new, old, name, .. }) => match (new, old) {
            (Some(new), Some(old)) => txn.update(&name, new, old, reason),
            (Some(new), None) => txn.create(&name, new, reason),
            (None, Some(old)) => txn.delete(&name, old, reason),
            _ => Ok(()),
        },
        BookmarkPush::Infinitepush(InfiniteBookmarkPush { name, new, old, .. }) => match (new, old)
        {
            (new, Some(old)) => txn.update_infinitepush(&name, new, old),
            (new, None) => txn.create_infinitepush(&name, new),
        },
    }
}

fn check_is_ancestor_opt(
    ctx: CoreContext,
    repo: BlobRepo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    old: Option<ChangesetId>,
    new: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    match old {
        None => ok(()).left_future(),
        Some(old) => {
            if old == new {
                ok(()).left_future()
            } else {
                lca_hint
                    .is_ancestor(ctx, repo.get_changeset_fetcher(), old, new)
                    .and_then(|is_ancestor| match is_ancestor {
                        true => Ok(()),
                        false => Err(format_err!("Non fastforward bookmark move")),
                    })
                    .right_future()
            }
        }
        .right_future(),
    }
}

fn hg_bookmark_push_to_bonsai(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark_push: BookmarkPush<HgChangesetId>,
) -> impl Future<Item = BookmarkPush<ChangesetId>, Error = Error> + Send {
    match bookmark_push {
        BookmarkPush::PlainPush(PlainBookmarkPush {
            part_id,
            name,
            old,
            new,
        }) => (
            bonsai_from_hg_opt(ctx.clone(), repo, old),
            bonsai_from_hg_opt(ctx, repo, new),
        )
            .into_future()
            .map(move |(old, new)| {
                BookmarkPush::PlainPush(PlainBookmarkPush {
                    part_id,
                    name,
                    old,
                    new,
                })
            })
            .left_future(),
        BookmarkPush::Infinitepush(InfiniteBookmarkPush {
            name,
            force,
            create,
            old,
            new,
        }) => (
            bonsai_from_hg_opt(ctx.clone(), repo, old),
            repo.get_bonsai_from_hg(ctx.clone(), new),
        )
            .into_future()
            .and_then(|(old, new)| match new {
                Some(new) => Ok((old, new)),
                None => Err(err_msg("Bonsai Changeset not found")),
            })
            .map(move |(old, new)| {
                BookmarkPush::Infinitepush(InfiniteBookmarkPush {
                    name,
                    force,
                    create,
                    old,
                    new,
                })
            })
            .right_future(),
    }
}

// TODO: (torozco) T44841164 bonsai_from_hg_opt should probably error if we map Some<HgChangesetId>
// to None.
fn bonsai_from_hg_opt(
    ctx: CoreContext,
    repo: &BlobRepo,
    cs_id: Option<HgChangesetId>,
) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
    match cs_id {
        None => ok(None).left_future(),
        Some(cs_id) => repo.get_bonsai_from_hg(ctx, cs_id).right_future(),
    }
}

fn log_commits_to_scribe(
    ctx: CoreContext,
    repo: BlobRepo,
    changesets: Vec<ChangesetId>,
    commit_scribe_category: Option<String>,
) -> BoxFuture<(), Error> {
    let queue = match commit_scribe_category {
        Some(category) => Arc::new(scribe_commit_queue::LogToScribe::new_with_default_scribe(
            ctx.fb, category,
        )),
        None => Arc::new(scribe_commit_queue::LogToScribe::new_with_discard()),
    };
    let futs = changesets.into_iter().map(move |changeset_id| {
        cloned!(ctx, repo, queue, changeset_id);
        let generation = repo
            .get_generation_number_by_bonsai(ctx.clone(), changeset_id)
            .and_then(|maybe_gen| maybe_gen.ok_or(err_msg("No generation number found")));
        let parents = repo.get_changeset_parents_by_bonsai(ctx, changeset_id);
        let repo_id = repo.get_repoid();
        let queue = queue;

        generation
            .join(parents)
            .and_then(move |(generation, parents)| {
                let ci = scribe_commit_queue::CommitInfo::new(
                    repo_id,
                    generation,
                    changeset_id,
                    parents,
                );
                queue.queue_commit(ci)
            })
    });
    future::join_all(futs).map(|_| ()).boxify()
}

fn run_hooks(
    ctx: CoreContext,
    changesets: Changesets,
    pushvars: Option<HashMap<String, Bytes>>,
    onto_bookmark: &BookmarkName,
    hook_manager: Arc<HookManager>,
) -> BoxFuture<(), BundleResolverError> {
    // TODO: should we also accept the Option<HgBookmarkPush> and run hooks on that?
    let mut futs = stream::FuturesUnordered::new();
    for (hg_cs_id, _) in changesets {
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
