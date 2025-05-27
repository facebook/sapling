/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Backsyncer
//!
//! Library to sync commits from source repo to target repo by following bookmark update log
//! and doing commit rewrites. The main motivation for backsyncer is to keep "small repo" up to
//! date with "large repo" in a setup where all writes to small repo are redirected to large repo
//! in a push redirector.
//! More details can be found here - <https://fb.quip.com/tZ4yAaA3S4Mc>
//!
//! Target repo tails source repo's bookmark update log and backsync bookmark updates one by one.
//! The latest backsynced log id is stored in mutable_counters table. Backsync consists of the
//! following phases:
//!
//! 1) Given an entry from bookmark update log of a target repo,
//!    find commits to backsync from source repo into a target repo.
//! 2) Rewrite these commits and create rewritten commits in target repo
//! 3) In the same transaction try to update a bookmark in the source repo AND latest backsynced
//!    log id.

use std::collections::HashSet;
use std::iter::once;
use std::iter::repeat;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use anyhow::Error;
use anyhow::bail;
use anyhow::format_err;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksArc;
use bookmarks::Freshness;
use cloned::cloned;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::sync_commit;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use futures::Future;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use futures_stats::TimedTryFutureExt;
use git_source_of_truth::GitSourceOfTruthConfig;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoConfigRef;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersArc;
use mutable_counters::SqlMutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;
use repo_update_logger::CommitInfo;
use repo_update_logger::find_draft_ancestors;
use repo_update_logger::log_bookmark_operation;
use repo_update_logger::log_new_commits;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::error;
use slog::info;
use slog::warn;
use sql::Transaction;
use sql_ext::TransactionResult;
use sql_query_config::SqlQueryConfig;
use thiserror::Error;
use wireproto_handler::TargetRepoDbs;

#[derive(Clone)]
#[facet::container]
pub struct Repo(
    dyn BonsaiHgMapping,
    dyn BonsaiGitMapping,
    dyn BonsaiGlobalrevMapping,
    dyn PushrebaseMutationMapping,
    RepoCrossRepo,
    RepoBookmarkAttrs,
    dyn Bookmarks,
    dyn BookmarkUpdateLog,
    FilestoreConfig,
    dyn MutableCounters,
    dyn Phases,
    RepoBlobstore,
    RepoConfig,
    RepoDerivedData,
    RepoIdentity,
    CommitGraph,
    dyn CommitGraphWriter,
    dyn Filenodes,
    SqlQueryConfig,
    dyn GitSourceOfTruthConfig,
);

#[cfg(test)]
mod tests;

#[derive(Debug, Error)]
pub enum BacksyncError {
    #[error("BacksyncError::LogEntryNotFound: {latest_log_id} not found")]
    LogEntryNotFound { latest_log_id: u64 },
    #[error("BacksyncError::Other")]
    Other(#[from] Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacksyncLimit {
    NoLimit,
    Limit(u64),
}

/// Block until a specific bookmark transaction (identified by its log id) is confirmed to be
/// backsynced.
///
/// The backsyncer tw jobs are responsible for backsyncing bookmark transactions.
/// When they do, they update a mutable counter to the id found in the bookmark update log for this
/// transaction.
/// Wait until the mutable counters indicate that backyncing caught up.
///
/// Note that we use some form of exponential backoff to avoid causing a thundering herd problem on
/// reading the mutable counters if the backsync is lagging
///
/// We also use a hard-coded timeout to avoid being stuck forever waiting for the backsync if it is
/// lagging. Not having this timeout has caused SEVs in the past, blocking lands.
pub async fn ensure_backsynced<R>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<R>,
    target_repo_dbs: Arc<TargetRepoDbs>,
    log_id: BookmarkUpdateLogId,
) -> Result<(), Error>
where
    R: RepoLike + Send + Sync + Clone + 'static,
{
    let timeout = Duration::from_secs(
        justknobs::get_as::<u64>(
            "scm/mononoke:defer_to_backsyncer_for_backsync_timeout_seconds",
            None,
        )
        .unwrap_or(60),
    );

    let source_repo_id = commit_syncer.get_source_repo().repo_identity().id();
    let counter_name = format_counter(&source_repo_id);
    let start_instant = Instant::now();

    let mut sleep_times = once(1)
        .chain(once(2))
        .chain(once(5))
        .chain(repeat(10))
        .map(Duration::from_secs);
    while start_instant.elapsed() < timeout {
        let counter: BookmarkUpdateLogId = target_repo_dbs
            .counters
            .get_counter(&ctx, &counter_name)
            .await?
            .unwrap_or(0)
            .try_into()?;
        if counter >= log_id {
            return Ok(());
        }
        tokio::time::sleep(
            sleep_times
                .next()
                .expect("sleep_times is an unbounded iterator"),
        )
        .await;
    }
    bail!("Timeout expired while waiting for backsyncing")
}

pub async fn backsync_latest<R>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<R>,
    target_repo_dbs: Arc<TargetRepoDbs>,
    limit: BacksyncLimit,
    cancellation_requested: Arc<AtomicBool>,
    sync_context: CommitSyncContext,
    disable_lease: bool,
    commit_only_backsync_future: Box<dyn Future<Output = ()> + Send + Unpin>,
) -> Result<Box<dyn Future<Output = ()> + Send + Unpin>, Error>
where
    R: RepoLike + Send + Sync + Clone + 'static,
{
    // TODO(ikostia): start borrowing `CommitSyncer`, no reason to consume it
    let large_repo_id = commit_syncer.get_source_repo().repo_identity().id();
    let counter_name = format_counter(&large_repo_id);

    let counter: BookmarkUpdateLogId = target_repo_dbs
        .counters
        .get_counter(&ctx, &counter_name)
        .await?
        .unwrap_or(0)
        .try_into()?;

    debug!(ctx.logger(), "fetched counter {}", &counter);

    let log_entries_limit = match limit {
        BacksyncLimit::Limit(limit) => limit,
        BacksyncLimit::NoLimit => {
            // Set limit extremely high to read all new values
            u64::MAX
        }
    };
    let next_entries: Vec<_> = commit_syncer
        .get_source_repo()
        .bookmark_update_log()
        .read_next_bookmark_log_entries(
            ctx.clone(),
            counter,
            log_entries_limit,
            Freshness::MostRecent,
        )
        .boxed()
        .try_collect()
        .await?;

    // Before syncing entries, check if cancellation has been
    // requested. If yes, then exit early.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!(ctx.logger(), "sync stopping due to cancellation request");
        return Ok(commit_only_backsync_future);
    }

    if next_entries.is_empty() {
        debug!(ctx.logger(), "nothing to sync");
        Ok(commit_only_backsync_future)
    } else {
        sync_entries(
            ctx,
            &commit_syncer,
            target_repo_dbs,
            next_entries,
            counter,
            cancellation_requested,
            sync_context,
            disable_lease,
            commit_only_backsync_future,
        )
        .boxed()
        .await
    }
}

async fn sync_entries<R>(
    mut ctx: CoreContext,
    commit_syncer: &CommitSyncer<R>,
    target_repo_dbs: Arc<TargetRepoDbs>,
    entries: Vec<BookmarkUpdateLogEntry>,
    mut counter: BookmarkUpdateLogId,
    cancellation_requested: Arc<AtomicBool>,
    sync_context: CommitSyncContext,
    disable_lease: bool,
    mut commit_only_backsync_future: Box<dyn Future<Output = ()> + Send + Unpin>,
) -> Result<Box<dyn Future<Output = ()> + Send + Unpin>, Error>
where
    R: RepoLike + Send + Sync + Clone + 'static,
{
    for entry in entries {
        // Before processing each entry, check if cancellation has
        // been requested and exit if that's the case.
        if cancellation_requested.load(Ordering::Relaxed) {
            info!(ctx.logger(), "sync stopping due to cancellation request");
            return Ok(commit_only_backsync_future);
        }
        let mut scuba_sample = ctx.scuba().clone();
        let pc = ctx.fork_perf_counters();
        let mut scuba_log_tag = "Backsyncing".to_string();
        let (stats, new_commit_only_backsync_future) = do_sync_entry(
            ctx.clone(),
            commit_syncer,
            &target_repo_dbs,
            entry,
            &mut counter,
            sync_context,
            disable_lease,
            commit_only_backsync_future,
            &mut scuba_sample,
            &mut scuba_log_tag,
        )
        .try_timed()
        .await?;
        commit_only_backsync_future = new_commit_only_backsync_future;
        pc.insert_perf_counters(&mut scuba_sample);
        scuba_sample
            .add_future_stats(&stats)
            .log_with_msg(&scuba_log_tag, None);
    }
    Ok(commit_only_backsync_future)
}

// This function is the inner function for sync_entries and shouldn't be called by other callers.
// It encapsulates of what we consider as doing a single "backsyncing" for bookmark entry: an
// activity that we want to time and log.
async fn do_sync_entry<R>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<R>,
    target_repo_dbs: &Arc<TargetRepoDbs>,
    entry: BookmarkUpdateLogEntry,
    counter: &mut BookmarkUpdateLogId,
    sync_context: CommitSyncContext,
    disable_lease: bool,
    mut commit_only_backsync_future: Box<dyn Future<Output = ()> + Send + Unpin>,
    scuba_sample: &mut MononokeScubaSampleBuilder,
    scuba_log_tag: &mut String,
) -> Result<Box<dyn Future<Output = ()> + Send + Unpin>, Error>
where
    R: RepoLike + Send + Sync + Clone + 'static,
{
    let entry_id = entry.id;
    if *counter >= entry_id {
        return Ok(commit_only_backsync_future);
    }
    debug!(ctx.logger(), "backsyncing {} ...", entry_id);

    if commit_syncer
        .rename_bookmark(&entry.bookmark_name)
        .await?
        .is_none()
    {
        // For the bookmarks that don't remap to small repos we can skip. But it's
        // still valuable to have commit mapping ready for them. That's why we spawn
        // a commit backsync future that we don't wait for here. Each of such futures
        // waits for result of previous commmit-only backsync so we don't duplicate
        // work unnecessarily.
        debug!(ctx.logger(), "Renamed bookmark is None. No sync happening.");
        target_repo_dbs
            .counters
            .set_counter(
                &ctx,
                &format_counter(&commit_syncer.get_source_repo().repo_identity().id()),
                entry.id.try_into()?,
                Some((*counter).try_into()?),
            )
            .await?;
        *counter = entry.id;
        if let Some(to_cs_id) = entry.to_changeset_id {
            commit_only_backsync_future = Box::new({
                cloned!(ctx, sync_context, to_cs_id, commit_syncer);
                mononoke::spawn_task(async move {
                    commit_only_backsync_future.await;
                    let res = sync_commit(
                        &ctx,
                        to_cs_id.clone(),
                        &commit_syncer,
                        // Backsyncer is always used in the large-to-small direction,
                        // therefore there can be at most one remapped candidate,
                        // so `CandidateSelectionHint::Only` is a safe choice
                        CandidateSelectionHint::Only,
                        sync_context,
                        disable_lease,
                    )
                    .await;
                    if let Err(err) = res {
                        error!(
                            ctx.logger(),
                            "Failed to backsync {} pointing to {}: {}",
                            entry.bookmark_name,
                            to_cs_id,
                            err
                        );
                    }
                })
                .map(|_| ())
            });
        }

        return Ok(commit_only_backsync_future);
    }

    scuba_sample.add("backsyncer_bookmark_log_entry_id", u64::from(entry.id));

    let start_instant = Instant::now();

    if let Some(to_cs_id) = entry.to_changeset_id {
        let (_, unsynced_ancestors_versions) =
            find_toposorted_unsynced_ancestors(&ctx, commit_syncer, to_cs_id, None).await?;

        if !unsynced_ancestors_versions.has_ancestor_with_a_known_outcome() {
            // Not a single ancestor of to_cs_id was ever synced.
            // That means that we can't figure out which commit sync mapping version
            // to use. In that case we just skip this entry and not sync it at all.
            // This seems the safest option (i.e. we won't rewrite a commit with
            // an incorrect version) but it also has a downside that the bookmark that points
            // to this commit is not going to be synced.
            warn!(
                ctx.logger(),
                "skipping {}, entry id {}", entry.bookmark_name, entry.id
            );
            *scuba_log_tag = "Skipping entry because there are no synced ancestors".to_string();
            target_repo_dbs
                .counters
                .set_counter(
                    &ctx,
                    &format_counter(&commit_syncer.get_source_repo().repo_identity().id()),
                    entry.id.try_into()?,
                    Some((*counter).try_into()?),
                )
                .await?;
            *counter = entry.id;
            return Ok(commit_only_backsync_future);
        }

        // Backsyncer is always used in the large-to-small direction,
        // therefore there can be at most one remapped candidate,
        // so `CandidateSelectionHint::Only` is a safe choice

        sync_commit(
            &ctx,
            to_cs_id,
            commit_syncer,
            CandidateSelectionHint::Only,
            sync_context,
            disable_lease,
        )
        .await?;
    }

    let new_counter = entry.id;
    let maybe_log_id = backsync_bookmark(
        ctx.clone(),
        commit_syncer,
        target_repo_dbs.clone(),
        Some(*counter),
        &entry,
    )
    .await?;
    scuba_sample.add_opt(
        "from_csid",
        entry.from_changeset_id.map(|csid| csid.to_string()),
    );
    scuba_sample.add_opt(
        "to_csid",
        entry.to_changeset_id.map(|csid| csid.to_string()),
    );
    scuba_sample.add(
        "backsync_duration_ms",
        u64::try_from(start_instant.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    scuba_sample.add("backsync_previously_done", maybe_log_id.is_none());

    if let Some(_log_id) = maybe_log_id {
        *counter = new_counter;
    } else {
        debug!(
            ctx.logger(),
            "failed to backsync {}, most likely another process already synced it ", entry_id
        );
        // Transaction failed, it could be because another process already backsynced it
        // Verify that counter was moved and continue if that's the case

        let source_repo_id = commit_syncer.get_source_repo().repo_identity().id();
        let counter_name = format_counter(&source_repo_id);
        let new_counter = target_repo_dbs
            .counters
            .get_counter(&ctx, &counter_name)
            .await?
            .unwrap_or(0)
            .try_into()?;
        if new_counter <= *counter {
            return Err(format_err!(
                "backsync transaction failed, but the counter didn't move forward. Was {}, became {}",
                *counter,
                new_counter,
            ));
        } else {
            debug!(
                ctx.logger(),
                "verified that another process has already synced {}", entry_id
            );
            *counter = new_counter;
        }
    }
    Ok(commit_only_backsync_future)
}

/// All "new" commits on this bookmark move. Use with care, creating a bookmark
/// means ALL ancestors are new.
async fn commits_added_by_bookmark_move(
    ctx: &CoreContext,
    repo: &impl RepoLike,
    from_cs_id: Option<ChangesetId>,
    to_cs_id: Option<ChangesetId>,
) -> Result<HashSet<ChangesetId>, Error> {
    match (from_cs_id, to_cs_id) {
        (_, None) => Ok(HashSet::new()),
        (None, Some(to_id)) => {
            repo.commit_graph()
                .ancestors_difference_stream(ctx, vec![to_id], vec![])
                .await?
                .try_collect()
                .await
        }
        (Some(from_id), Some(to_id)) => {
            // If needed, this can be optimised by using range_stream when from_id is
            // an ancestor of from_id
            repo.commit_graph()
                .ancestors_difference_stream(ctx, vec![to_id], vec![from_id])
                .await?
                .try_collect()
                .await
        }
    }
}

async fn backsync_bookmark<R>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<R>,
    target_repo_dbs: Arc<TargetRepoDbs>,
    prev_counter: Option<BookmarkUpdateLogId>,
    log_entry: &BookmarkUpdateLogEntry,
) -> Result<Option<BookmarkUpdateLogId>, Error>
where
    R: RepoLike + Send + Sync + Clone + 'static,
{
    let prev_counter: Option<i64> = prev_counter.map(|x| x.try_into()).transpose()?;
    let target_repo_id = commit_syncer.get_target_repo().repo_identity().id();
    let source_repo_id = commit_syncer.get_source_repo().repo_identity().id();
    debug!(ctx.logger(), "preparing to backsync {:?}", log_entry);

    let new_counter = log_entry.id;
    let bookmark = commit_syncer
        .rename_bookmark(&log_entry.bookmark_name)
        .await?;
    debug!(ctx.logger(), "bookmark was renamed into {:?}", bookmark);
    let from_cs_id = log_entry.from_changeset_id;
    let to_cs_id = log_entry.to_changeset_id;

    let get_commit_sync_outcome = |maybe_cs_id: Option<ChangesetId>| {
        cloned!(ctx);
        async move {
            match maybe_cs_id {
                Some(cs_id) => {
                    let maybe_outcome = commit_syncer.get_commit_sync_outcome(&ctx, cs_id).await?;
                    match maybe_outcome {
                        Some(outcome) => Ok(Some((outcome, cs_id))),
                        None => Err(format_err!("{} hasn't been backsynced yet", cs_id)),
                    }
                }
                None => Ok(None),
            }
        }
    };

    let get_remapped_cs_id =
        move |maybe_outcome: Option<(CommitSyncOutcome, ChangesetId)>| match maybe_outcome {
            Some((outcome, cs_id)) => {
                use CommitSyncOutcome::*;
                match outcome {
                    NotSyncCandidate(_) => Err(format_err!(
                        "invalid bookmark move: {:?} should not be synced to target repo",
                        cs_id
                    )),
                    RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => {
                        Ok(Some(cs_id))
                    }
                }
            }
            None => Ok(None),
        };

    if let Some(bookmark) = bookmark {
        // Fetch sync outcome before transaction to keep transaction as short as possible
        let from_sync_outcome = get_commit_sync_outcome(from_cs_id).await?;
        let to_sync_outcome = get_commit_sync_outcome(to_cs_id).await?;
        debug!(
            ctx.logger(),
            "commit sync outcomes: from_cs: {:?}, to_cs: {:?}", from_sync_outcome, to_sync_outcome
        );

        let from_cs_id = get_remapped_cs_id(from_sync_outcome)?;
        let to_cs_id = get_remapped_cs_id(to_sync_outcome)?;
        let bookmark_info = BookmarkInfo {
            bookmark_name: bookmark.clone(),
            bookmark_kind: BookmarkKind::Publishing, // Only publishing bookmarks are back synced
            operation: BookmarkOperation::new(from_cs_id, to_cs_id)?,
            reason: BookmarkUpdateReason::Backsyncer,
        };

        if from_cs_id != to_cs_id {
            let target_repo = commit_syncer.get_target_repo();
            // This CANNOT be done after getting the bookmark transaction, because it accesses SQL without a
            // transaction and that causes a deadlock that blocks the syncing.
            let globalrev_entries: Vec<BonsaiGlobalrevMappingEntry> = if target_repo
                .repo_config()
                .pushrebase
                .globalrev_config
                .as_ref()
                .map(|c| &c.publishing_bookmark)
                == Some(&bookmark)
            {
                let all_commits =
                    commits_added_by_bookmark_move(&ctx, target_repo, from_cs_id, to_cs_id).await?;
                let ctx = &ctx;
                let blobstore = target_repo.repo_blobstore();
                stream::iter(all_commits)
                    .map(|bcs_id| async move {
                        let cs = bcs_id.load(ctx, blobstore).await?;
                        // When pushrebasing into the large repo, this commit
                        // should've gotten a globalrev
                        // But if it doesn't it's better to skip assigning the globalrev
                        // than to break the backsync process. (The commit was already acknowledged
                        // and we have not much choice on whether to sync it).
                        let globalrev = Globalrev::from_bcs(&cs).ok();
                        if let Some(globalrev) = globalrev {
                            anyhow::Ok(Some(BonsaiGlobalrevMappingEntry { bcs_id, globalrev }))
                        } else {
                            anyhow::Ok(None)
                        }
                    })
                    .buffer_unordered(100)
                    .try_filter_map(|res| future::ready(Ok(res)))
                    .try_collect()
                    .await?
            } else {
                vec![]
            };
            let commits_to_log = async {
                match to_cs_id {
                    Some(to_cs_id) => {
                        let res = find_draft_ancestors(&ctx, target_repo, to_cs_id).await;
                        match res {
                            Ok(bcss) => bcss.iter().map(|bcs| CommitInfo::new(bcs, None)).collect(),
                            Err(err) => {
                                ctx.scuba().clone().log_with_msg(
                                    "Failed to find draft ancestors for logging",
                                    Some(format!("{}", err)),
                                );
                                vec![]
                            }
                        }
                    }
                    None => vec![],
                }
            }
            .await;

            let mut bookmark_txn = target_repo_dbs.bookmarks.create_transaction(ctx.clone());
            debug!(
                ctx.logger(),
                "syncing bookmark {} to {:?}", bookmark, to_cs_id
            );

            match (from_cs_id, to_cs_id) {
                (Some(from), Some(to)) => {
                    debug!(
                        ctx.logger(),
                        "updating bookmark {:?} from {:?} to {:?}", bookmark, from, to
                    );
                    bookmark_txn.update(&bookmark, to, from, BookmarkUpdateReason::Backsyncer)?;
                }
                (Some(from), None) => {
                    debug!(
                        ctx.logger(),
                        "deleting bookmark {:?} with original position {:?}", bookmark, from
                    );
                    bookmark_txn.delete(&bookmark, from, BookmarkUpdateReason::Backsyncer)?;
                }
                (None, Some(to)) => {
                    debug!(
                        ctx.logger(),
                        "creating bookmark {:?} to point to {:?}", bookmark, to
                    );
                    bookmark_txn.create(&bookmark, to, BookmarkUpdateReason::Backsyncer)?;
                }
                (None, None) => {
                    bail!("unexpected bookmark move");
                }
            };
            let new_counter = new_counter.try_into()?;

            let txn_hook = Arc::new({
                move |ctx: CoreContext, txn: Transaction| {
                    cloned!(globalrev_entries);
                    async move {
                        // This is an abstraction leak: it only works because the
                        // mutable counters/globalrevs are stored in the same db as the
                        // bookmarks.
                        let txn_result = SqlMutableCounters::set_counter_on_txn(
                            &ctx,
                            target_repo_id,
                            &format_counter(&source_repo_id),
                            new_counter,
                            prev_counter,
                            txn,
                        )
                        .await?;

                        let txn = match txn_result {
                            TransactionResult::Succeeded(txn) => Ok(txn),
                            TransactionResult::Failed => Err(BookmarkTransactionError::LogicError),
                        }?;

                        if !globalrev_entries.is_empty() {
                            bonsai_globalrev_mapping::add_globalrevs(
                                &ctx,
                                txn,
                                target_repo_id,
                                &globalrev_entries,
                            )
                            .await
                            .map_err(|err| BookmarkTransactionError::Other(err.into()))
                        } else {
                            Ok(txn)
                        }
                    }
                    .boxed()
                }
            });

            let res = bookmark_txn
                .commit_with_hooks(vec![txn_hook])
                .await?
                .map(|x| x.into());
            log_new_commits(
                &ctx,
                target_repo,
                Some((&bookmark, BookmarkKind::Publishing)),
                commits_to_log,
            )
            .await;
            log_bookmark_operation(&ctx, target_repo, &bookmark_info).await;
            return Ok(res);
        } else {
            debug!(
                ctx.logger(),
                "from_cs_id and to_cs_id are the same: {:?}. No sync happening for {:?}",
                from_cs_id,
                bookmark
            );
        }
    } else {
        debug!(ctx.logger(), "Renamed bookmark is None. No sync happening.");
    }

    let maybe_log_id = if target_repo_dbs
        .counters
        .set_counter(
            &ctx,
            &format_counter(&source_repo_id),
            new_counter.try_into()?,
            prev_counter,
        )
        .await?
    {
        Some(new_counter)
    } else {
        None
    };

    Ok(maybe_log_id)
}

pub async fn open_backsyncer_dbs(repo: &impl RepoLike) -> Result<TargetRepoDbs, Error> {
    Ok(TargetRepoDbs {
        bookmarks: repo.bookmarks_arc(),
        bookmark_update_log: repo.bookmark_update_log_arc(),
        counters: repo.mutable_counters_arc(),
    })
}

pub fn format_counter(repo_to_backsync_from: &RepositoryId) -> String {
    format!("backsync_from_{}", repo_to_backsync_from.id())
}
