/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    BundleResolverError, InfiniteBookmarkPush, NonFastForwardPolicy, PlainBookmarkPush,
    PostResolveAction, PostResolveBookmarkOnlyPushRebase, PostResolveInfinitePush, PostResolvePush,
    PostResolvePushRebase, PushrebaseBookmarkSpec,
};
use anyhow::{anyhow, format_err, Context, Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_git_mapping::{
    bulk_add_git_mapping_in_transaction, extract_git_sha1_from_bonsai_extra, BonsaiGitMappingEntry,
};
use bookmarks::{
    BookmarkName, BookmarkUpdateReason, BundleReplayData, Transaction, TransactionHook,
};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::try_join,
    stream::{FuturesOrdered, FuturesUnordered, TryStreamExt},
    FutureExt, StreamExt, TryFutureExt,
};
use futures_ext::{try_boxfuture, FutureExt as OldFutureExt};
use futures_stats::TimedFutureExt;
use git_mapping_pushrebase_hook::GitMappingPushrebaseHook;
use globalrev_pushrebase_hook::GlobalrevPushrebaseHook;
use maplit::hashset;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, PushrebaseParams};
use mononoke_types::{BonsaiChangeset, ChangesetId, RawBundle2Id, RepositoryId};
use pushrebase::{self, PushrebaseHook};
use reachabilityindex::LeastCommonAncestorsHint;
use reverse_filler_queue::ReverseFillerQueue;
use scribe_commit_queue::{self, ScribeCommitQueue};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, o, warn};
use stats::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tunables::tunables;

use crate::rate_limits::enforce_commit_rate_limits;
use crate::response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};

enum BookmarkPush<T: Copy> {
    PlainPush(PlainBookmarkPush<T>),
    Infinitepush(InfiniteBookmarkPush<T>),
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
    maybe_reverse_filler_queue: Option<&dyn ReverseFillerQueue>,
    action: PostResolveAction,
) -> Result<UnbundleResponse, BundleResolverError> {
    enforce_commit_rate_limits(ctx.clone(), &action)
        .compat()
        .await?;

    // FIXME: it's used not only in pushrebase, so it worth moving
    // populate_git_mapping outside of PushrebaseParams.
    let populate_git_mapping = pushrebase_params.populate_git_mapping;
    let unbundle_response = match action {
        PostResolveAction::Push(action) => run_push(
            ctx,
            repo,
            bookmark_attrs,
            lca_hint,
            infinitepush_params,
            action,
            populate_git_mapping,
        )
        .await
        .context("While doing a push")
        .map(UnbundleResponse::Push)?,
        PostResolveAction::InfinitePush(action) => run_infinitepush(
            ctx,
            repo,
            lca_hint,
            infinitepush_params,
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
            action,
            populate_git_mapping,
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
    action: PostResolvePush,
    populate_git_mapping: bool,
) -> Result<UnbundlePushResponse, Error> {
    debug!(ctx.logger(), "unbundle processing: running push.");
    let PostResolvePush {
        changegroup_id,
        bookmark_pushes,
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

    let bookmark_ids = bookmark_pushes.iter().map(|bp| bp.part_id).collect();
    let reason = BookmarkUpdateReason::Push {
        bundle_replay_data: maybe_raw_bundle2_id.map(BundleReplayData::new),
    };

    let bookmark_pushes_futures: FuturesUnordered<_> = bookmark_pushes
        .into_iter()
        .map({
            |bookmark_push| async {
                check_plain_bookmark_push_allowed(
                    &ctx,
                    &repo,
                    &bookmark_attrs,
                    non_fast_forward_policy,
                    &infinitepush_params,
                    bookmark_push,
                    lca_hint,
                )
                .await
            }
        })
        .collect();

    let uploaded_bonsais: HashMap<_, _> = uploaded_bonsais
        .into_iter()
        .map(|bcs| (bcs.get_changeset_id(), bcs))
        .collect();

    let repo_id = repo.get_repoid();
    let bookmark_pushes = bookmark_pushes_futures.try_collect::<Vec<_>>().await?;
    let mut txn_hook = None;
    if populate_git_mapping {
        let parents_of_uploaded =
            check_bookmark_pushes_for_git_mirrors(&bookmark_pushes, &uploaded_bonsais)?;
        let ancestors_no_git_mapping =
            find_ancestors_without_git_mapping(&ctx, &repo, parents_of_uploaded).await?;

        txn_hook = Some(upload_git_mapping_bookmark_txn_hook(
            repo_id,
            uploaded_bonsais,
            ancestors_no_git_mapping,
        ));
    }

    let bookmark_pushes = bookmark_pushes
        .into_iter()
        .map(|bp| Some(BookmarkPush::PlainPush(bp)))
        .collect::<Vec<_>>();

    save_bookmark_pushes_to_db(ctx, repo, reason, bookmark_pushes, txn_hook).await?;
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
                    .compat()
                    .map_err(Error::from);

                let mapping_fut = repo.bonsai_git_mapping().get(cs_id.into());

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
    repo_id: RepositoryId,
    uploaded_bonsais: HashMap<ChangesetId, BonsaiChangeset>,
    ancestors_no_git_mapping: HashMap<ChangesetId, BonsaiChangeset>,
) -> TransactionHook {
    Arc::new(move |ctx, sql_txn| {
        let uploaded_bonsais_len = uploaded_bonsais.len();
        let ancestors_no_git_mapping_len = ancestors_no_git_mapping.len();

        let mut mapping_entries = vec![];
        for (bcs_id, bonsai) in uploaded_bonsais
            .iter()
            .chain(ancestors_no_git_mapping.iter())
        {
            let maybe_git_sha1 = try_boxfuture!(extract_git_sha1_from_bonsai_extra(bonsai.extra()));
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

        async move {
            let sql_txn = bulk_add_git_mapping_in_transaction(sql_txn, &repo_id, &mapping_entries)
                .map_err(Error::from)
                .await?;
            ctx.scuba()
                .clone()
                .log_with_msg("Inserted git mapping", None);
            Ok(sql_txn)
        }
        .boxed()
        .compat()
        .boxify()
    })
}

// To keep things simple we allow only a trivial push case
// 1) Single bookmark
// 2) all uploaded commits are reachable from this bookmark
//
// This function returns parents of `uploaded_bonsais` that are not in
// uploaded_bonsais
fn check_bookmark_pushes_for_git_mirrors(
    bookmark_pushes: &[PlainBookmarkPush<ChangesetId>],
    uploaded_bonsais: &HashMap<ChangesetId, BonsaiChangeset>,
) -> Result<HashSet<ChangesetId>, Error> {
    let only_single_book_err = anyhow!(
        "only pushes of a single bookmark are allowed for repos that are mirrored from git repos"
    );

    if bookmark_pushes.len() != 1 {
        return Err(only_single_book_err);
    }

    let bookmark_push = bookmark_pushes
        .iter()
        .next()
        .ok_or_else(|| only_single_book_err)?;

    let new = match bookmark_push.new {
        Some(new) => new,
        None => {
            if !uploaded_bonsais.is_empty() {
                return Err(anyhow!(
                    "pushing new commits is not allowed with bookmark deletion"
                ));
            }
            return Ok(HashSet::new());
        }
    };

    // Do a bfs search starting from `new` to check if all changesets are found
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    if let Some(new_bcs) = uploaded_bonsais.get(&new) {
        queue.push_back(new_bcs);
        visited.insert(new);
    }
    let mut outside_parents = HashSet::new();
    while let Some(bcs) = queue.pop_back() {
        for p in bcs.parents() {
            if let Some(bcs) = uploaded_bonsais.get(&p) {
                if !visited.insert(p) {
                    continue;
                }
                queue.push_back(bcs);
            } else {
                outside_parents.insert(p);
            }
        }
    }

    if visited.len() != uploaded_bonsais.len() {
        return Err(anyhow!(
            "Some of the pushed commits are not reachable from the bookmark. reachable: {}, uploaded: {}",
            visited.len(),
            uploaded_bonsais.len()
        ));
    }

    Ok(outside_parents)
}

async fn run_infinitepush(
    ctx: &CoreContext,
    repo: &BlobRepo,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    maybe_reverse_filler_queue: Option<&dyn ReverseFillerQueue>,
    action: PostResolveInfinitePush,
) -> Result<UnbundleInfinitePushResponse, Error> {
    debug!(ctx.logger(), "unbundle processing: running infinitepush");
    let PostResolveInfinitePush {
        changegroup_id,
        maybe_bookmark_push,
        mutations,
        maybe_raw_bundle2_id,
        uploaded_bonsais: _,
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

    let bookmark_push = match maybe_bookmark_push {
        Some(bookmark_push) => bookmark_push,
        None => {
            // Changegroup was saved during bundle2 resolution
            // there's nothing we need to do here.
            return Ok(UnbundleInfinitePushResponse { changegroup_id });
        }
    };

    let reason = BookmarkUpdateReason::Push {
        bundle_replay_data: maybe_raw_bundle2_id.map(BundleReplayData::new),
    };

    let maybe_bonsai_bookmark_push = filter_or_check_infinitepush_allowed(
        ctx,
        repo,
        lca_hint,
        infinitepush_params,
        bookmark_push,
    )
    .await
    .context("While verifying Infinite Push bookmark push")
    .map(|maybe_bp| maybe_bp.map(BookmarkPush::Infinitepush))?;
    save_bookmark_pushes_to_db(ctx, repo, reason, vec![maybe_bonsai_bookmark_push], None).await?;
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

    // TODO: (dbudischek) T41565649 log pushed changesets as well, not only pushrebased
    let new_commits = pushrebased_changesets.iter().map(|p| p.id_new).collect();

    log_commits_to_scribe(
        ctx,
        repo,
        &bookmark,
        new_commits,
        pushrebase_params.commit_scribe_category.clone(),
    )
    .await?;

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
    action: PostResolveBookmarkOnlyPushRebase,
    populate_git_mapping: bool,
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
    let reason = BookmarkUpdateReason::Pushrebase {
        // Since this a bookmark-only pushrebase, there are no changeset timestamps
        bundle_replay_data: maybe_raw_bundle2_id.map(|id| BundleReplayData::new(id)),
    };

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
    if populate_git_mapping {
        if let Some(new) = bookmark_push.new {
            let ancestors_no_git_mapping =
                find_ancestors_without_git_mapping(&ctx, &repo, hashset! {new}).await?;
            txn_hook = Some(upload_git_mapping_bookmark_txn_hook(
                repo.get_repoid(),
                HashMap::new(),
                ancestors_no_git_mapping,
            ));
        }
    }

    let maybe_bookmark_push = Some(BookmarkPush::PlainPush(bookmark_push));
    save_bookmark_pushes_to_db(ctx, repo, reason, vec![maybe_bookmark_push], txn_hook).await?;
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
                .add("pushrebase_retry_num", res.retry_num)
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
    let reason = BookmarkUpdateReason::Pushrebase {
        bundle_replay_data: maybe_hg_replay_data
            .as_ref()
            .map(|hg_replay_data| hg_replay_data.get_raw_bundle2_id())
            .map(BundleReplayData::new),
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

    save_bookmark_pushes_to_db(ctx, repo, reason, vec![maybe_bookmark_push], None).await?;

    // Note that this push did not do any actual rebases, so we do not
    // need to provide any actual mapping, an empty Vec will do
    Ok((target_bcs, Vec::new()))
}

/// Save several bookmark pushes to the database
async fn save_bookmark_pushes_to_db<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    reason: BookmarkUpdateReason,
    bonsai_bookmark_pushes: Vec<Option<BookmarkPush<ChangesetId>>>,
    txn_hook: Option<TransactionHook>,
) -> Result<(), Error> {
    if bonsai_bookmark_pushes.is_empty() {
        // If we have no bookmarks, then don't create an empty transaction. This is a
        // temporary workaround for the fact that we committing an empty transaction
        // evicts the cache.
        return Ok(());
    }

    let mut txn = repo.update_bookmark_transaction(ctx.clone());

    for bp in bonsai_bookmark_pushes.into_iter().flatten() {
        add_bookmark_to_transaction(&mut txn, bp, reason.clone())?;
    }

    let ok = if let Some(txn_hook) = txn_hook {
        txn.commit_with_hook(txn_hook).compat().await?
    } else {
        txn.commit().compat().await?
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

/// Run sanity checks for infinitepush bookmark moves
async fn filter_or_check_infinitepush_allowed(
    ctx: &CoreContext,
    repo: &BlobRepo,
    lca_hint: &dyn LeastCommonAncestorsHint,
    infinitepush_params: &InfinitepushParams,
    bp: InfiniteBookmarkPush<ChangesetId>,
) -> Result<Option<InfiniteBookmarkPush<ChangesetId>>, Error> {
    match &infinitepush_params.namespace {
        Some(namespace) => {
            // First, check that we match the namespace.
            if !namespace.matches_bookmark(&bp.name) {
                return Err(format_err!(
                    "Invalid Infinitepush bookmark: {} (Infinitepush bookmarks must match pattern {})",
                    &bp.name,
                    namespace.as_str()
                ));
            }
            // Now, check that the bookmark we want to update either exists or is being created.
            if !(bp.old.is_some() || bp.create) {
                let e = format_err!("Unknown bookmark: {}. Use --create to create one.", bp.name);
                return Err(e);
            }
            // Finally, check that the push is a fast-forward, or --force is set.
            if !bp.force {
                check_is_ancestor_opt(ctx, repo, lca_hint, bp.old, bp.new)
                    .await
                    .map_err(|e| format_err!("{} (try --force?)", e))?
            }
            Ok(Some(bp))
        }
        None => {
            warn!(ctx.logger(), "Infinitepush bookmark push to {} was ignored", bp.name; o!("remote" => "true"));
            Ok(None)
        }
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
                .is_ancestor(ctx.clone(), repo.get_changeset_fetcher(), old, new)
                .compat()
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
    bookmark: &BookmarkName,
    changesets: Vec<ChangesetId>,
    commit_scribe_category: Option<String>,
) -> Result<(), Error> {
    let queue = match commit_scribe_category {
        Some(category) => {
            scribe_commit_queue::LogToScribe::new_with_default_scribe(ctx.fb, category)
        }
        None => scribe_commit_queue::LogToScribe::new_with_discard(),
    };

    let repo_id = repo.get_repoid();
    let bookmark = bookmark.as_str();

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
                );
                queue.queue_commit(&ci).await
            }
        })
        .collect();
    futs.try_for_each(|()| async { Ok(()) }).await
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
        let hook = GitMappingPushrebaseHook::new(repo.get_repoid());
        hooks.push(hook);
    }

    hooks
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobstore::Loadable;
    use bonsai_git_mapping::{CONVERT_REVISION_EXTRA, HGGIT_SOURCE_EXTRA};
    use fbinit::FacebookInit;
    use fixtures::linear;
    use maplit::{hashmap, hashset};
    use mononoke_types::hash::GitSha1;
    use mononoke_types_mocks::hash::{ONES_GIT_SHA1, TWOS_GIT_SHA1};
    use tests_utils::{resolve_cs_id, CreateCommitContext};

    #[fbinit::compat_test]
    async fn test_check_bookmark_pushes_for_git_mirrors(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        let parent_of_master_cs_id =
            resolve_cs_id(&ctx, &repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a").await?;
        let cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        // Moving a single bookmark to already existing commit - should be allowed
        let res = check_bookmark_pushes_for_git_mirrors(
            &[PlainBookmarkPush {
                part_id: 0,
                name: BookmarkName::new("master")?,
                old: None,
                new: Some(cs_id),
            }],
            &HashMap::new(),
        );
        assert!(res.is_ok());
        assert_eq!(res?, hashset! {});

        // Moving two bookmarks should fail
        let res = check_bookmark_pushes_for_git_mirrors(
            &[
                PlainBookmarkPush {
                    part_id: 0,
                    name: BookmarkName::new("master")?,
                    old: None,
                    new: Some(cs_id),
                },
                PlainBookmarkPush {
                    part_id: 0,
                    name: BookmarkName::new("master2")?,
                    old: None,
                    new: Some(cs_id),
                },
            ],
            &HashMap::new(),
        );
        assert!(res.is_err());

        // Single bookmark to a single new commit - should be allowed
        let master_bcs = cs_id.load(ctx.clone(), repo.blobstore()).compat().await?;

        let res = check_bookmark_pushes_for_git_mirrors(
            &[PlainBookmarkPush {
                part_id: 0,
                name: BookmarkName::new("master")?,
                old: None,
                new: Some(cs_id),
            }],
            &hashmap! {
                cs_id => master_bcs.clone(),
            },
        );
        assert!(res.is_ok());
        assert_eq!(res?, hashset! {parent_of_master_cs_id});

        // Single bookmark with two new commits - should be allowed
        let parent_of_master_bcs = parent_of_master_cs_id
            .load(ctx.clone(), repo.blobstore())
            .compat()
            .await?;

        let parent_of_parent_of_master_cs_id =
            resolve_cs_id(&ctx, &repo, "3c15267ebf11807f3d772eb891272b911ec68759").await?;
        let res = check_bookmark_pushes_for_git_mirrors(
            &[PlainBookmarkPush {
                part_id: 0,
                name: BookmarkName::new("master")?,
                old: None,
                new: Some(cs_id),
            }],
            &hashmap! {
                cs_id => master_bcs.clone(),
                parent_of_master_cs_id => parent_of_master_bcs.clone(),
            },
        );
        assert!(res.is_ok());
        assert_eq!(res?, hashset! {parent_of_parent_of_master_cs_id});

        // Single bookmark with one unrelated commit - should be disallowed
        let unrelated_cs_id = "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";
        let unrelated_cs_id = resolve_cs_id(&ctx, &repo, unrelated_cs_id).await?;
        let unrelated_bcs = unrelated_cs_id.load(ctx, repo.blobstore()).compat().await?;

        let res = check_bookmark_pushes_for_git_mirrors(
            &[PlainBookmarkPush {
                part_id: 0,
                name: BookmarkName::new("master")?,
                old: None,
                new: Some(cs_id),
            }],
            &hashmap! {
                cs_id => master_bcs,
                parent_of_master_cs_id => parent_of_master_bcs,
                unrelated_cs_id => unrelated_bcs,
            },
        );
        assert!(res.is_err());

        Ok(())
    }

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
            .bulk_add(&[BonsaiGitMappingEntry {
                git_sha1: ONES_GIT_SHA1,
                bcs_id: parent,
            }])
            .await?;

        let res = find_ancestors_without_git_mapping(&ctx, &repo, hashset! {child}).await?;
        assert_eq!(res.keys().collect::<HashSet<_>>(), hashset![&child]);

        repo.bonsai_git_mapping()
            .bulk_add(&[BonsaiGitMappingEntry {
                git_sha1: TWOS_GIT_SHA1,
                bcs_id: child,
            }])
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
