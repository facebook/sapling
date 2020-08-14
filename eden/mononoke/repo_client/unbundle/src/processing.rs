/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    BundleResolverError, NonFastForwardPolicy, PlainBookmarkPush, PostResolveAction,
    PostResolveBookmarkOnlyPushRebase, PostResolveInfinitePush, PostResolvePush,
    PostResolvePushRebase, PushrebaseBookmarkSpec,
};
use anyhow::{anyhow, format_err, Context, Error, Result};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::{BookmarkName, BookmarkUpdateReason, BundleReplay};
use bookmarks_movement::{BookmarkUpdatePolicy, BookmarkUpdateTargets};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::try_join,
    stream::{FuturesUnordered, TryStreamExt},
};
use futures_stats::TimedFutureExt;
use git_mapping_pushrebase_hook::GitMappingPushrebaseHook;
use globalrev_pushrebase_hook::GlobalrevPushrebaseHook;
use mercurial_bundle_replay_data::BundleReplayData;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushParams, PushrebaseParams};
use mononoke_types::{BonsaiChangeset, ChangesetId, RawBundle2Id};
use pushrebase::PushrebaseHook;
use reachabilityindex::LeastCommonAncestorsHint;
use reverse_filler_queue::ReverseFillerQueue;
use scribe_commit_queue::{self, LogToScribe};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, warn};
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use tunables::tunables;

use crate::rate_limits::enforce_commit_rate_limits;
use crate::response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};

define_stats! {
    prefix = "mononoke.unbundle.processed";
    push: dynamic_timeseries("{}.push", (reponame: String); Rate, Sum),
    pushrebase: dynamic_timeseries("{}.pushrebase", (reponame: String); Rate, Sum),
    bookmark_only_pushrebase: dynamic_timeseries("{}.bookmark_only_pushrebase", (reponame: String); Rate, Sum),
    infinitepush: dynamic_timeseries("{}.infinitepush", (reponame: String); Rate, Sum),
}

pub async fn run_post_resolve_action(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
    push_params: &PushParams,
    maybe_reverse_filler_queue: Option<&dyn ReverseFillerQueue>,
    action: PostResolveAction,
) -> Result<UnbundleResponse, BundleResolverError> {
    enforce_commit_rate_limits(ctx.clone(), &action)
        .compat()
        .await?;

    // FIXME: it's used not only in pushrebase, so it worth moving
    // populate_git_mapping outside of PushrebaseParams.
    let unbundle_response = match action {
        PostResolveAction::Push(action) => run_push(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            pushrebase_params,
            action,
            push_params,
        )
        .await
        .context("While doing a push")
        .map(UnbundleResponse::Push)?,
        PostResolveAction::InfinitePush(action) => run_infinitepush(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            pushrebase_params,
            maybe_reverse_filler_queue,
            action,
        )
        .await
        .context("While doing an infinitepush")
        .map(UnbundleResponse::InfinitePush)?,
        PostResolveAction::PushRebase(action) => run_pushrebase(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            pushrebase_params,
            action,
        )
        .await
        .map(UnbundleResponse::PushRebase)?,
        PostResolveAction::BookmarkOnlyPushRebase(action) => run_bookmark_only_pushrebase(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            pushrebase_params,
            action,
        )
        .await
        .context("While doing a bookmark-only pushrebase")
        .map(UnbundleResponse::BookmarkOnlyPushRebase)?,
    };
    report_unbundle_type(&repo, &unbundle_response);
    Ok(unbundle_response)
}

fn report_unbundle_type(repo: &BlobRepo, unbundle_response: &UnbundleResponse) {
    let repo_name = repo.name().clone();
    match unbundle_response {
        UnbundleResponse::Push(_) => STATS::push.add_value(1, (repo_name,)),
        UnbundleResponse::PushRebase(_) => STATS::pushrebase.add_value(1, (repo_name,)),
        UnbundleResponse::InfinitePush(_) => STATS::infinitepush.add_value(1, (repo_name,)),
        UnbundleResponse::BookmarkOnlyPushRebase(_) => {
            STATS::bookmark_only_pushrebase.add_value(1, (repo_name,))
        }
    }
}

async fn run_push(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
    action: PostResolvePush,
    push_params: &PushParams,
) -> Result<UnbundlePushResponse, Error> {
    debug!(ctx.logger(), "unbundle processing: running push.");
    let PostResolvePush {
        changegroup_id,
        mut bookmark_pushes,
        mutations,
        maybe_raw_bundle2_id,
        non_fast_forward_policy,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
    } = action;

    if tunables().get_mutation_accept_for_infinitepush() {
        repo.hg_mutation_store()
            .add_entries(ctx, uploaded_hg_changeset_ids, mutations)
            .await
            .context("Failed to store mutation data")?;
    }

    if bookmark_pushes.len() > 1 {
        return Err(anyhow!(
            "only push to at most one bookmark is allowed, got {:?}",
            bookmark_pushes
        ));
    }

    let mut new_changeset_ids = Vec::new();
    let mut new_changesets = HashMap::new();
    for bcs in uploaded_bonsais {
        let cs_id = bcs.get_changeset_id();
        new_changeset_ids.push(cs_id);
        new_changesets.insert(cs_id, bcs);
    }

    let mut bookmark_ids = Vec::new();
    let mut maybe_bookmark = None;
    if let Some(bookmark_push) = bookmark_pushes.pop() {
        bookmark_ids.push(bookmark_push.part_id);
        let bundle_replay_data = maybe_raw_bundle2_id.map(BundleReplayData::new);
        let bundle_replay_data = bundle_replay_data
            .as_ref()
            .map(|data| data as &dyn BundleReplay);

        match (bookmark_push.old, bookmark_push.new) {
            (None, Some(new_target)) => {
                bookmarks_movement::CreateBookmarkOp::new(
                    &bookmark_push.name,
                    new_target,
                    BookmarkUpdateReason::Push,
                )
                .only_if_public()
                .with_new_changesets(new_changesets)
                .with_bundle_replay_data(bundle_replay_data)
                .run(
                    ctx,
                    repo,
                    infinitepush_params,
                    pushrebase_params,
                    bookmark_attrs,
                )
                .await
                .context("Failed to create bookmark")?;
            }

            (Some(old_target), Some(new_target)) => {
                bookmarks_movement::UpdateBookmarkOp::new(
                    &bookmark_push.name,
                    BookmarkUpdateTargets {
                        old: old_target,
                        new: new_target,
                    },
                    if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                        BookmarkUpdatePolicy::AnyPermittedByConfig
                    } else {
                        BookmarkUpdatePolicy::FastForwardOnly
                    },
                    BookmarkUpdateReason::Push,
                )
                .only_if_public()
                .with_new_changesets(new_changesets)
                .with_bundle_replay_data(bundle_replay_data)
                .run(
                    ctx,
                    repo,
                    lca_hint,
                    infinitepush_params,
                    pushrebase_params,
                    bookmark_attrs,
                )
                .await
                .context(
                    if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                        "Failed to move bookmark"
                    } else {
                        "Failed to fast-forward bookmark (try --force?)"
                    },
                )?;
            }

            (Some(old_target), None) => {
                bookmarks_movement::DeleteBookmarkOp::new(
                    &bookmark_push.name,
                    old_target,
                    BookmarkUpdateReason::Push,
                )
                .only_if_public()
                .with_bundle_replay_data(bundle_replay_data)
                .run(ctx, repo, infinitepush_params, bookmark_attrs)
                .await
                .context("Failed to delete bookmark")?;
            }

            (None, None) => {}
        }

        maybe_bookmark = Some(bookmark_push.name);
    }

    log_commits_to_scribe(
        ctx,
        repo,
        maybe_bookmark.as_ref(),
        new_changeset_ids,
        push_params.commit_scribe_category.clone(),
    )
    .await;

    Ok(UnbundlePushResponse {
        changegroup_id,
        bookmark_ids,
    })
}

async fn save_to_reverse_filler_queue(
    ctx: &CoreContext,
    reponame: &String,
    maybe_reverse_filler_queue: Option<&dyn ReverseFillerQueue>,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
) -> Result<(), Error> {
    if let Some(reverse_filler_queue) = maybe_reverse_filler_queue {
        if let Some(ref raw_bundle2_id) = maybe_raw_bundle2_id {
            debug!(
                ctx.logger(),
                "saving infinitepush bundle {:?} into the reverse filler queue", raw_bundle2_id
            );
            reverse_filler_queue
                .insert_bundle(reponame, raw_bundle2_id)
                .await?;
            ctx.scuba()
                .clone()
                .log_with_msg("Saved into ReverseFillerQueue", None);
        } else {
            warn!(
                ctx.logger(),
                "reverse filler queue enabled, but bundle preservation is not!"
            );
        }
    }

    Ok(())
}

async fn run_infinitepush(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
    maybe_reverse_filler_queue: Option<&dyn ReverseFillerQueue>,
    action: PostResolveInfinitePush,
) -> Result<UnbundleInfinitePushResponse, Error> {
    debug!(ctx.logger(), "unbundle processing: running infinitepush");
    let PostResolveInfinitePush {
        changegroup_id,
        maybe_bookmark_push,
        mutations,
        maybe_raw_bundle2_id,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
        is_cross_backend_sync,
    } = action;

    if !is_cross_backend_sync {
        save_to_reverse_filler_queue(
            ctx,
            repo.name(),
            maybe_reverse_filler_queue,
            maybe_raw_bundle2_id,
        )
        .await?;
    }

    if tunables().get_mutation_accept_for_infinitepush() {
        repo.hg_mutation_store()
            .add_entries(ctx, uploaded_hg_changeset_ids, mutations)
            .await
            .context("Failed to store mutation data")?;
    }

    let bookmark = match maybe_bookmark_push {
        Some(bookmark_push) => {
            let bundle_replay_data = maybe_raw_bundle2_id.map(BundleReplayData::new);
            let bundle_replay_data = bundle_replay_data
                .as_ref()
                .map(|data| data as &dyn BundleReplay);
            if bookmark_push.old.is_none() && bookmark_push.create {
                bookmarks_movement::CreateBookmarkOp::new(
                    &bookmark_push.name,
                    bookmark_push.new,
                    BookmarkUpdateReason::Push,
                )
                .only_if_scratch()
                .with_bundle_replay_data(bundle_replay_data)
                .run(
                    ctx,
                    repo,
                    infinitepush_params,
                    pushrebase_params,
                    bookmark_attrs,
                )
                .await
                .context("Failed to create scratch bookmark")?;
            } else {
                let old_target = bookmark_push.old.ok_or_else(|| {
                    anyhow!(
                        "Unknown bookmark: {}. Use --create to create one.",
                        bookmark_push.name
                    )
                })?;
                bookmarks_movement::UpdateBookmarkOp::new(
                    &bookmark_push.name,
                    BookmarkUpdateTargets {
                        old: old_target,
                        new: bookmark_push.new,
                    },
                    if bookmark_push.force {
                        BookmarkUpdatePolicy::AnyPermittedByConfig
                    } else {
                        BookmarkUpdatePolicy::FastForwardOnly
                    },
                    BookmarkUpdateReason::Push,
                )
                .only_if_scratch()
                .with_bundle_replay_data(bundle_replay_data)
                .run(
                    ctx,
                    repo,
                    lca_hint,
                    infinitepush_params,
                    pushrebase_params,
                    bookmark_attrs,
                )
                .await
                .context(if bookmark_push.force {
                    "Failed to move scratch bookmark"
                } else {
                    "Failed to fast-forward scratch bookmark (try --force?)"
                })?;
            }

            Some(bookmark_push.name)
        }
        None => None,
    };

    let new_commits: Vec<ChangesetId> = uploaded_bonsais
        .iter()
        .map(|cs| cs.get_changeset_id())
        .collect();

    log_commits_to_scribe(
        ctx,
        repo,
        bookmark.as_ref(),
        new_commits,
        infinitepush_params.commit_scribe_category.clone(),
    )
    .await;

    Ok(UnbundleInfinitePushResponse { changegroup_id })
}

async fn run_pushrebase(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
    action: PostResolvePushRebase,
) -> Result<UnbundlePushRebaseResponse, BundleResolverError> {
    debug!(ctx.logger(), "unbundle processing: running pushrebase.");
    let PostResolvePushRebase {
        any_merges,
        bookmark_push_part_id,
        bookmark_spec,
        maybe_hg_replay_data,
        maybe_pushvars: _,
        commonheads,
        uploaded_bonsais,
    } = action;

    // FIXME: stop cloning when this fn is async
    let bookmark = bookmark_spec.get_bookmark_name().clone();

    let (pushrebased_rev, pushrebased_changesets) = match bookmark_spec {
        // There's no `.context()` after `normal_pushrebase`, as it has
        // `Error=BundleResolverError` and doing `.context("bla").from_err()`
        // would turn some useful variant of `BundleResolverError` into generic
        // `BundleResolverError::Error`, which in turn would render incorrectly
        // (see definition of `BundleResolverError`).
        PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark) => {
            normal_pushrebase(
                ctx,
                repo,
                &pushrebase_params,
                &uploaded_bonsais,
                any_merges,
                &onto_bookmark,
                &maybe_hg_replay_data,
                bookmark_attrs,
                infinitepush_params,
            )
            .await?
        }
        PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => force_pushrebase(
            ctx,
            repo,
            &pushrebase_params,
            lca_hint,
            uploaded_bonsais,
            plain_push,
            &maybe_hg_replay_data,
            bookmark_attrs,
            infinitepush_params,
        )
        .await
        .context("While doing a force pushrebase")?,
    };

    repo.get_phases()
        .add_reachable_as_public(ctx.clone(), vec![pushrebased_rev.clone()])
        .compat()
        .await
        .context("While marking pushrebased changeset as public")?;

    let new_commits = pushrebased_changesets.iter().map(|p| p.id_new).collect();

    log_commits_to_scribe(
        ctx,
        repo,
        Some(&bookmark),
        new_commits,
        pushrebase_params.commit_scribe_category.clone(),
    )
    .await;

    Ok(UnbundlePushRebaseResponse {
        commonheads,
        pushrebased_rev,
        pushrebased_changesets,
        onto: bookmark,
        bookmark_push_part_id,
    })
}

async fn run_bookmark_only_pushrebase(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
    action: PostResolveBookmarkOnlyPushRebase,
) -> Result<UnbundleBookmarkOnlyPushRebaseResponse, Error> {
    debug!(
        ctx.logger(),
        "unbundle processing: running bookmark-only pushrebase."
    );
    let PostResolveBookmarkOnlyPushRebase {
        bookmark_push,
        maybe_raw_bundle2_id,
        non_fast_forward_policy,
    } = action;

    let part_id = bookmark_push.part_id;
    let bundle_replay_data = maybe_raw_bundle2_id.map(BundleReplayData::new);
    let bundle_replay_data = bundle_replay_data
        .as_ref()
        .map(|data| data as &dyn BundleReplay);

    match (bookmark_push.old, bookmark_push.new) {
        (None, Some(new_target)) => {
            bookmarks_movement::CreateBookmarkOp::new(
                &bookmark_push.name,
                new_target,
                BookmarkUpdateReason::Pushrebase,
            )
            .only_if_public()
            .with_bundle_replay_data(bundle_replay_data)
            .run(
                ctx,
                repo,
                infinitepush_params,
                pushrebase_params,
                bookmark_attrs,
            )
            .await
            .context("Failed to create bookmark")?;
        }

        (Some(old_target), Some(new_target)) => {
            bookmarks_movement::UpdateBookmarkOp::new(
                &bookmark_push.name,
                BookmarkUpdateTargets {
                    old: old_target,
                    new: new_target,
                },
                if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                    BookmarkUpdatePolicy::AnyPermittedByConfig
                } else {
                    BookmarkUpdatePolicy::FastForwardOnly
                },
                BookmarkUpdateReason::Pushrebase,
            )
            .only_if_public()
            .with_bundle_replay_data(bundle_replay_data)
            .run(
                ctx,
                repo,
                lca_hint,
                infinitepush_params,
                pushrebase_params,
                bookmark_attrs,
            )
            .await
            .context(
                if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                    "Failed to move bookmark"
                } else {
                    "Failed to fast-forward bookmark (try --force?)"
                },
            )?;
        }

        (Some(old_target), None) => {
            bookmarks_movement::DeleteBookmarkOp::new(
                &bookmark_push.name,
                old_target,
                BookmarkUpdateReason::Pushrebase,
            )
            .only_if_public()
            .with_bundle_replay_data(bundle_replay_data)
            .run(ctx, repo, infinitepush_params, bookmark_attrs)
            .await
            .context("Failed to delete bookmark")?;
        }

        (None, None) => {}
    }

    Ok(UnbundleBookmarkOnlyPushRebaseResponse {
        bookmark_push_part_id: part_id,
    })
}

async fn normal_pushrebase(
    ctx: &CoreContext,
    repo: &BlobRepo,
    pushrebase_params: &PushrebaseParams,
    changesets: &HashSet<BonsaiChangeset>,
    any_merges: bool,
    bookmark: &BookmarkName,
    maybe_hg_replay_data: &Option<pushrebase::HgReplayData>,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<(ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), BundleResolverError> {
    check_plain_bookmark_move_preconditions(
        &ctx,
        &bookmark,
        "pushrebase",
        &bookmark_attrs,
        &infinitepush_params,
    )?;

    let block_merges = pushrebase_params.block_merges.clone();
    if block_merges && any_merges {
        return Err(format_err!(
            "Pushrebase blocked because it contains a merge commit.\n\
             If you need this for a specific use case please contact\n\
             the Source Control team at https://fburl.com/27qnuyl2"
        )
        .into());
    }

    let hooks = get_pushrebase_hooks(&repo, &pushrebase_params);

    let mut flags = pushrebase_params.flags.clone();
    if let Some(rewritedates) = bookmark_attrs.should_rewrite_dates(bookmark) {
        // Bookmark config overrides repo flags.rewritedates config
        flags.rewritedates = rewritedates;
    }

    ctx.scuba().clone().log_with_msg("Pushrebase started", None);
    let (stats, result) = pushrebase::do_pushrebase_bonsai(
        &ctx,
        &repo,
        &flags,
        bookmark,
        &changesets,
        maybe_hg_replay_data.as_ref(),
        &hooks[..],
    )
    .timed()
    .await;

    let mut scuba_logger = ctx.scuba().clone();
    scuba_logger.add_future_stats(&stats);

    match result {
        Ok(ref res) => {
            scuba_logger
                .add("pushrebase_retry_num", res.retry_num.0)
                .log_with_msg("Pushrebase finished", None);
        }
        Err(ref err) => {
            scuba_logger.log_with_msg("Pushrebase failed", Some(format!("{:#?}", err)));
        }
    }

    result
        .map_err(|err| match err {
            pushrebase::PushrebaseError::Conflicts(conflicts) => {
                BundleResolverError::PushrebaseConflicts(conflicts)
            }
            _ => BundleResolverError::Error(format_err!("pushrebase failed {:?}", err)),
        })
        .map(|res| (res.head, res.rebased_changesets))
}

async fn force_pushrebase(
    ctx: &CoreContext,
    repo: &BlobRepo,
    pushrebase_params: &PushrebaseParams,
    lca_hint: &dyn LeastCommonAncestorsHint,
    uploaded_bonsais: HashSet<BonsaiChangeset>,
    bookmark_push: PlainBookmarkPush<ChangesetId>,
    maybe_hg_replay_data: &Option<pushrebase::HgReplayData>,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<(ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), Error> {
    let new_target = bookmark_push
        .new
        .ok_or_else(|| anyhow!("new changeset is required for force pushrebase"))?;

    let mut new_changeset_ids = Vec::new();
    let mut new_changesets = HashMap::new();
    for bcs in uploaded_bonsais {
        let cs_id = bcs.get_changeset_id();
        new_changeset_ids.push(cs_id);
        new_changesets.insert(cs_id, bcs);
    }

    let bundle_replay_data = if let Some(hg_replay_data) = &maybe_hg_replay_data {
        Some(hg_replay_data.to_bundle_replay_data(None).await?)
    } else {
        None
    };
    let bundle_replay_data = bundle_replay_data
        .as_ref()
        .map(|data| data as &dyn BundleReplay);

    match bookmark_push.old {
        None => {
            bookmarks_movement::CreateBookmarkOp::new(
                &bookmark_push.name,
                new_target,
                BookmarkUpdateReason::Pushrebase,
            )
            .only_if_public()
            .with_new_changesets(new_changesets)
            .with_bundle_replay_data(bundle_replay_data)
            .run(
                ctx,
                repo,
                infinitepush_params,
                pushrebase_params,
                bookmark_attrs,
            )
            .await
            .context("Failed to create bookmark")?;
        }

        Some(old_target) => {
            bookmarks_movement::UpdateBookmarkOp::new(
                &bookmark_push.name,
                BookmarkUpdateTargets {
                    old: old_target,
                    new: new_target,
                },
                BookmarkUpdatePolicy::AnyPermittedByConfig,
                BookmarkUpdateReason::Pushrebase,
            )
            .only_if_public()
            .with_new_changesets(new_changesets)
            .with_bundle_replay_data(bundle_replay_data)
            .run(
                ctx,
                repo,
                lca_hint,
                infinitepush_params,
                pushrebase_params,
                bookmark_attrs,
            )
            .await
            .context("Failed to move bookmark")?;
        }
    }

    log_commits_to_scribe(
        ctx,
        repo,
        Some(&bookmark_push.name),
        new_changeset_ids,
        pushrebase_params.commit_scribe_category.clone(),
    )
    .await;

    // Note that this push did not do any actual rebases, so we do not
    // need to provide any actual mapping, an empty Vec will do
    Ok((new_target, Vec::new()))
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

async fn log_commits_to_scribe(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: Option<&BookmarkName>,
    changesets: Vec<ChangesetId>,
    commit_scribe_category: Option<String>,
) {
    let queue = match commit_scribe_category {
        Some(category) if !category.is_empty() => LogToScribe::new(ctx.scribe().clone(), category),
        _ => LogToScribe::new_with_discard(),
    };

    let repo_id = repo.get_repoid();
    let bookmark = bookmark.map(|bm| bm.as_str());

    let futs: FuturesUnordered<_> = changesets
        .into_iter()
        .map(|changeset_id| {
            let queue = &queue;
            async move {
                let get_generation = async {
                    repo.get_generation_number(ctx.clone(), changeset_id)
                        .compat()
                        .await?
                        .ok_or_else(|| Error::msg("No generation number found"))
                };
                let get_parents = async {
                    repo.get_changeset_parents_by_bonsai(ctx.clone(), changeset_id)
                        .compat()
                        .await
                };

                let (generation, parents) = try_join(get_generation, get_parents).await?;

                let ci = scribe_commit_queue::CommitInfo::new(
                    repo_id,
                    bookmark,
                    generation,
                    changeset_id,
                    parents,
                    ctx.user_unix_name().as_deref(),
                    ctx.source_hostname().as_deref(),
                );
                queue.queue_commit(&ci)
            }
        })
        .collect();
    let res = futs.try_for_each(|()| async { Ok(()) }).await;
    if let Err(err) = res {
        ctx.scuba()
            .clone()
            .log_with_msg("Failed to log pushed commits", Some(format!("{}", err)));
    }
}

/// Get a Vec of the relevant pushrebase hooks for PushrebaseParams, using this BlobRepo when
/// required by those hooks.
pub fn get_pushrebase_hooks(
    repo: &BlobRepo,
    params: &PushrebaseParams,
) -> Vec<Box<dyn PushrebaseHook>> {
    let mut hooks = vec![];

    if params.assign_globalrevs {
        let hook = GlobalrevPushrebaseHook::new(
            repo.bonsai_globalrev_mapping().clone(),
            repo.get_repoid(),
        );
        hooks.push(hook);
    }

    if params.populate_git_mapping {
        let hook = GitMappingPushrebaseHook::new(repo.bonsai_git_mapping().clone());
        hooks.push(hook);
    }

    hooks
}
