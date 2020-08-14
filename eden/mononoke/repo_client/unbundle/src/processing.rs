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
use blobstore::Loadable;
use bonsai_git_mapping::{
    extract_git_sha1_from_bonsai_extra, BonsaiGitMapping, BonsaiGitMappingEntry,
};
use bookmarks::{
    BookmarkName, BookmarkTransaction, BookmarkTransactionHook, BookmarkUpdateReason, BundleReplay,
};
use bookmarks_movement::{BookmarkUpdatePolicy, BookmarkUpdateTargets};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::try_join,
    stream::{FuturesOrdered, FuturesUnordered, TryStreamExt},
    FutureExt, StreamExt, TryFutureExt,
};
use futures_stats::TimedFutureExt;
use git_mapping_pushrebase_hook::GitMappingPushrebaseHook;
use globalrev_pushrebase_hook::GlobalrevPushrebaseHook;
use maplit::hashset;
use mercurial_bundle_replay_data::BundleReplayData;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushParams, PushrebaseParams};
use mononoke_types::{BonsaiChangeset, ChangesetId, RawBundle2Id};
use pushrebase::{self, PushrebaseHook};
use reachabilityindex::LeastCommonAncestorsHint;
use reverse_filler_queue::ReverseFillerQueue;
use scribe_commit_queue::{self, LogToScribe};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, warn};
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tunables::tunables;

use crate::rate_limits::enforce_commit_rate_limits;
use crate::response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};

enum BookmarkPush<T: Copy> {
    PlainPush(PlainBookmarkPush<T>),
}

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

/// Return ancestors of `start` which have git mapping extras but do not
/// have git mapping entry set in db.
async fn find_ancestors_without_git_mapping(
    ctx: &CoreContext,
    repo: &BlobRepo,
    start: HashSet<ChangesetId>,
) -> Result<HashMap<ChangesetId, BonsaiChangeset>, Error> {
    let mut res = HashMap::new();

    let mut visited = HashSet::new();
    let mut queue = FuturesOrdered::new();
    let mut get_new_queue_entry = |cs_id: ChangesetId| {
        if visited.insert(cs_id) {
            Some(async move {
                let bcs_fut = cs_id
                    .load(ctx.clone(), &repo.get_blobstore())
                    .map_err(Error::from);

                let mapping_fut = repo.bonsai_git_mapping().get(ctx, cs_id.into());

                let (bcs, git_mapping) = try_join(bcs_fut, mapping_fut).await?;
                Result::<_, Error>::Ok((cs_id, bcs, git_mapping))
            })
        } else {
            None
        }
    };

    for cs_id in start {
        if let Some(entry) = get_new_queue_entry(cs_id) {
            queue.push(entry)
        }
    }

    while let Some(entry) = queue.next().await {
        let (cs_id, bcs, git_mapping) = entry?;
        if !git_mapping.is_empty() {
            continue;
        }

        // Don't traverse past commits that do not have git sha1 set
        // This is done deliberately to avoid retraversing these commits over
        // and over.
        if extract_git_sha1_from_bonsai_extra(bcs.extra())?.is_none() {
            continue;
        }

        for p in bcs.parents() {
            if let Some(entry) = get_new_queue_entry(p) {
                queue.push(entry)
            }
        }
        res.insert(cs_id, bcs);
    }

    Ok(res)
}

fn upload_git_mapping_bookmark_txn_hook(
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    uploaded_bonsais: HashMap<ChangesetId, BonsaiChangeset>,
    ancestors_no_git_mapping: HashMap<ChangesetId, BonsaiChangeset>,
) -> BookmarkTransactionHook {
    Arc::new(move |ctx, sql_txn| {
        let uploaded_bonsais_len = uploaded_bonsais.len();
        let ancestors_no_git_mapping_len = ancestors_no_git_mapping.len();

        let mut mapping_entries = vec![];
        for (bcs_id, bonsai) in uploaded_bonsais
            .iter()
            .chain(ancestors_no_git_mapping.iter())
        {
            let maybe_git_sha1 = match extract_git_sha1_from_bonsai_extra(bonsai.extra()) {
                Ok(r) => r,
                Err(e) => return async move { Err(e.into()) }.boxed(),
            };
            if let Some(git_sha1) = maybe_git_sha1 {
                let entry = BonsaiGitMappingEntry {
                    git_sha1,
                    bcs_id: *bcs_id,
                };
                mapping_entries.push(entry);
            }
        }

        // Normally we expect git_mapping_new_changesets == git_mapping_inserting
        // and git_mapping_ancestors_no_mapping == 0.
        ctx.scuba()
            .clone()
            .add("git_mapping_new_changesets", uploaded_bonsais_len)
            .add(
                "git_mapping_ancestors_no_mapping",
                ancestors_no_git_mapping_len,
            )
            .add("git_mapping_inserting", mapping_entries.len())
            .log_with_msg("Inserting git mapping", None);

        let bonsai_git_mapping = bonsai_git_mapping.clone();
        async move {
            let sql_txn = bonsai_git_mapping
                .bulk_add_git_mapping_in_transaction(&ctx, &mapping_entries, sql_txn)
                .map_err(Error::from)
                .await?;
            ctx.scuba()
                .clone()
                .log_with_msg("Inserted git mapping", None);
            Ok(sql_txn)
        }
        .boxed()
    })
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
        PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => {
            normal_pushrebase(
                ctx,
                repo,
                &pushrebase_params,
                &uploaded_bonsais,
                any_merges,
                &onto_params,
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
    let reason = BookmarkUpdateReason::Pushrebase;
    // Since this a bookmark-only pushrebase, there are no changeset timestamps
    let bundle_replay_data = maybe_raw_bundle2_id.map(BundleReplayData::new);

    let bookmark_push = check_plain_bookmark_push_allowed(
        ctx,
        repo,
        bookmark_attrs,
        non_fast_forward_policy,
        infinitepush_params,
        bookmark_push,
        lca_hint,
    )
    .await?;

    let mut txn_hook = None;
    if pushrebase_params.populate_git_mapping {
        if let Some(new) = bookmark_push.new {
            let ancestors_no_git_mapping =
                find_ancestors_without_git_mapping(&ctx, &repo, hashset! {new}).await?;
            txn_hook = Some(upload_git_mapping_bookmark_txn_hook(
                repo.bonsai_git_mapping().clone(),
                HashMap::new(),
                ancestors_no_git_mapping,
            ));
        }
    }

    let maybe_bookmark_push = Some(BookmarkPush::PlainPush(bookmark_push));
    save_bookmark_pushes_to_db(
        ctx,
        repo,
        reason,
        &bundle_replay_data,
        vec![maybe_bookmark_push],
        txn_hook,
    )
    .await?;
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
    onto_bookmark: &pushrebase::OntoBookmarkParams,
    maybe_hg_replay_data: &Option<pushrebase::HgReplayData>,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<(ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), BundleResolverError> {
    let bookmark = &onto_bookmark.bookmark;

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
        &onto_bookmark,
        &changesets,
        maybe_hg_replay_data,
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
    bookmark_push: PlainBookmarkPush<ChangesetId>,
    maybe_hg_replay_data: &Option<pushrebase::HgReplayData>,
    bookmark_attrs: &BookmarkAttrs,
    infinitepush_params: &InfinitepushParams,
) -> Result<(ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), Error> {
    if pushrebase_params.assign_globalrevs {
        return Err(Error::msg(
            "force_pushrebase is not allowed when assigning Globalrevs",
        ));
    }
    if pushrebase_params.populate_git_mapping {
        return Err(Error::msg(
            "force_pushrebase is not allowed as it would skip populating Git mappings",
        ));
    }

    let maybe_target_bcs = bookmark_push.new.clone();
    let target_bcs = maybe_target_bcs
        .ok_or_else(|| Error::msg("new changeset is required for force pushrebase"))?;
    let reason = BookmarkUpdateReason::Pushrebase;
    let bundle_replay_data = if let Some(hg_replay_data) = &maybe_hg_replay_data {
        Some(hg_replay_data.to_bundle_replay_data(None).await?)
    } else {
        None
    };

    let maybe_bookmark_push = check_plain_bookmark_push_allowed(
        ctx,
        repo,
        bookmark_attrs,
        NonFastForwardPolicy::Allowed,
        infinitepush_params,
        bookmark_push,
        lca_hint,
    )
    .await
    .map(|bp| Some(BookmarkPush::PlainPush(bp)))?;

    save_bookmark_pushes_to_db(
        ctx,
        repo,
        reason,
        &bundle_replay_data,
        vec![maybe_bookmark_push],
        None,
    )
    .await?;

    // Note that this push did not do any actual rebases, so we do not
    // need to provide any actual mapping, an empty Vec will do
    Ok((target_bcs, Vec::new()))
}

/// Save several bookmark pushes to the database
async fn save_bookmark_pushes_to_db<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    reason: BookmarkUpdateReason,
    bundle_replay_data: &'a Option<BundleReplayData>,
    bonsai_bookmark_pushes: Vec<Option<BookmarkPush<ChangesetId>>>,
    txn_hook: Option<BookmarkTransactionHook>,
) -> Result<(), Error> {
    if bonsai_bookmark_pushes.is_empty() {
        // If we have no bookmarks, then don't create an empty transaction. This is a
        // temporary workaround for the fact that we committing an empty transaction
        // evicts the cache.
        return Ok(());
    }

    let mut txn = repo.update_bookmark_transaction(ctx.clone());

    for bp in bonsai_bookmark_pushes.into_iter().flatten() {
        add_bookmark_to_transaction(&mut txn, bp, reason, bundle_replay_data)?;
    }

    let ok = if let Some(txn_hook) = txn_hook {
        txn.commit_with_hook(txn_hook).await?
    } else {
        txn.commit().await?
    };

    if ok {
        Ok(())
    } else {
        Err(format_err!("Boookmark transaction failed"))
    }
}

/// Run sanity checks for plain (non-infinitepush) bookmark pushes
async fn check_plain_bookmark_push_allowed(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_attrs: &BookmarkAttrs,
    non_fast_forward_policy: NonFastForwardPolicy,
    infinitepush_params: &InfinitepushParams,
    bp: PlainBookmarkPush<ChangesetId>,
    lca_hint: &dyn LeastCommonAncestorsHint,
) -> Result<PlainBookmarkPush<ChangesetId>, Error> {
    check_plain_bookmark_move_preconditions(
        &ctx,
        &bp.name,
        "push",
        &bookmark_attrs,
        &infinitepush_params,
    )?;

    let fastforward_only_bookmark = bookmark_attrs.is_fast_forward_only(&bp.name);
    // only allow non fast forward moves if the pushvar is set and the bookmark does not
    // explicitly block them.
    let block_non_fast_forward =
        fastforward_only_bookmark || non_fast_forward_policy == NonFastForwardPolicy::Disallowed;

    match (bp.old, bp.new) {
        (old, Some(new)) if block_non_fast_forward => {
            check_is_ancestor_opt(ctx, repo, lca_hint, old, new)
                .await
                .map(|_| bp)
        }
        (Some(_old), None) if fastforward_only_bookmark => Err(format_err!(
            "Deletion of bookmark {} is forbidden.",
            bp.name
        )),
        _ => Ok(bp),
    }
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
    txn: &mut Box<dyn BookmarkTransaction>,
    bookmark_push: BookmarkPush<ChangesetId>,
    reason: BookmarkUpdateReason,
    bundle_replay_data: &Option<BundleReplayData>,
) -> Result<()> {
    match bookmark_push {
        BookmarkPush::PlainPush(PlainBookmarkPush { new, old, name, .. }) => {
            let bundle_replay = bundle_replay_data
                .as_ref()
                .map(|data| data as &dyn BundleReplay);
            match (new, old) {
                (Some(new), Some(old)) => txn.update(&name, new, old, reason, bundle_replay),
                (Some(new), None) => txn.create(&name, new, reason, bundle_replay),
                (None, Some(old)) => txn.delete(&name, old, reason, bundle_replay),
                _ => Ok(()),
            }
        }
    }
}

async fn check_is_ancestor_opt(
    ctx: &CoreContext,
    repo: &BlobRepo,
    lca_hint: &dyn LeastCommonAncestorsHint,
    old: Option<ChangesetId>,
    new: ChangesetId,
) -> Result<(), Error> {
    if let Some(old) = old {
        if old != new {
            let is_ancestor = lca_hint
                .is_ancestor(ctx, &repo.get_changeset_fetcher(), old, new)
                .await?;
            if !is_ancestor {
                return Err(format_err!(
                    "Non fastforward bookmark move from {} to {}",
                    old,
                    new
                ));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bonsai_git_mapping::{CONVERT_REVISION_EXTRA, HGGIT_SOURCE_EXTRA};
    use fbinit::FacebookInit;
    use fixtures::linear;
    use maplit::hashset;
    use mononoke_types::hash::GitSha1;
    use mononoke_types_mocks::hash::{ONES_GIT_SHA1, TWOS_GIT_SHA1};
    use tests_utils::CreateCommitContext;

    #[fbinit::compat_test]
    async fn test_find_ancestors_without_git_mapping_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        fn add_git_extras(context: CreateCommitContext, hash: GitSha1) -> CreateCommitContext {
            context
                .add_extra(
                    CONVERT_REVISION_EXTRA.to_string(),
                    format!("{}", hash).as_bytes().to_vec(),
                )
                .add_extra(HGGIT_SOURCE_EXTRA.to_string(), b"git".to_vec())
        };

        let parent = add_git_extras(CreateCommitContext::new_root(&ctx, &repo), ONES_GIT_SHA1)
            .commit()
            .await?;

        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {parent}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![&parent]);

        let child = add_git_extras(
            CreateCommitContext::new(&ctx, &repo, vec![parent]),
            TWOS_GIT_SHA1,
        )
        .commit()
        .await?;

        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {child}).await?;
        assert_eq!(
            res.keys().collect::<HashSet<_>>(),
            hashset![&parent, &child]
        );

        repo.bonsai_git_mapping()
            .bulk_add(
                &ctx,
                &[BonsaiGitMappingEntry {
                    git_sha1: ONES_GIT_SHA1,
                    bcs_id: parent,
                }],
            )
            .await?;

        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {child}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![&child]);

        repo.bonsai_git_mapping()
            .bulk_add(
                &ctx,
                &[BonsaiGitMappingEntry {
                    git_sha1: TWOS_GIT_SHA1,
                    bcs_id: child,
                }],
            )
            .await?;
        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {child}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![]);

        Ok(())
    }
    #[fbinit::compat_test]
    async fn test_find_ancestors_without_git_mapping_no_extras(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        let parent = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![]);

        let child = CreateCommitContext::new(&ctx, &repo, vec![parent])
            .commit()
            .await?;
        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {child}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![]);

        Ok(())
    }
}
