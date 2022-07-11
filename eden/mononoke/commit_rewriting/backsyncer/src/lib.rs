/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Backsyncer
///
/// Library to sync commits from source repo to target repo by following bookmark update log
/// and doing commit rewrites. The main motivation for backsyncer is to keep "small repo" up to
/// date with "large repo" in a setup where all writes to small repo are redirected to large repo
/// in a push redirector.
/// More details can be found here - <https://fb.quip.com/tZ4yAaA3S4Mc>
///
/// Target repo tails source repo's bookmark update log and backsync bookmark updates one by one.
/// The latest backsynced log id is stored in mutable_counters table. Backsync consists of the
/// following phases:
///
/// 1) Given an entry from bookmark update log of a target repo,
///    find commits to backsync from source repo into a target repo.
/// 2) Rewrite these commits and create rewritten commits in target repo
/// 3) In the same transaction try to update a bookmark in the source repo AND latest backsynced
///    log id.
use anyhow::bail;
/// Backsyncer
///
/// Library to sync commits from source repo to target repo by following bookmark update log
/// and doing commit rewrites. The main motivation for backsyncer is to keep "small repo" up to
/// date with "large repo" in a setup where all writes to small repo are redirected to large repo
/// in a push redirector.
/// More details can be found here - <https://fb.quip.com/tZ4yAaA3S4Mc>
///
/// Target repo tails source repo's bookmark update log and backsync bookmark updates one by one.
/// The latest backsynced log id is stored in mutable_counters table. Backsync consists of the
/// following phases:
///
/// 1) Given an entry from bookmark update log of a target repo,
///    find commits to backsync from source repo into a target repo.
/// 2) Rewrite these commits and create rewritten commits in target repo
/// 3) In the same transaction try to update a bookmark in the source repo AND latest backsynced
///    log id.
use anyhow::format_err;
/// Backsyncer
///
/// Library to sync commits from source repo to target repo by following bookmark update log
/// and doing commit rewrites. The main motivation for backsyncer is to keep "small repo" up to
/// date with "large repo" in a setup where all writes to small repo are redirected to large repo
/// in a push redirector.
/// More details can be found here - <https://fb.quip.com/tZ4yAaA3S4Mc>
///
/// Target repo tails source repo's bookmark update log and backsync bookmark updates one by one.
/// The latest backsynced log id is stored in mutable_counters table. Backsync consists of the
/// following phases:
///
/// 1) Given an entry from bookmark update log of a target repo,
///    find commits to backsync from source repo into a target repo.
/// 2) Rewrite these commits and create rewritten commits in target repo
/// 3) In the same transaction try to update a bookmark in the source repo AND latest backsynced
///    log id.
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore_factory::make_metadata_sql_factory;
use blobstore_factory::ReadOnlyStorage;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Freshness;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use futures::FutureExt;
use futures::TryStreamExt;
use metaconfig_types::MetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mutable_counters::ArcMutableCounters;
use mutable_counters::MutableCountersArc;
use mutable_counters::SqlMutableCounters;
use slog::debug;
use slog::info;
use slog::warn;
use sql::Transaction;
use sql_ext::facebook::MysqlOptions;
use sql_ext::SqlConnections;
use sql_ext::TransactionResult;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use synced_commit_mapping::SyncedCommitMapping;
use thiserror::Error;

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

pub async fn backsync_latest<M>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<M>,
    target_repo_dbs: TargetRepoDbs,
    limit: BacksyncLimit,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    // TODO(ikostia): start borrowing `CommitSyncer`, no reason to consume it
    let source_repo_id = commit_syncer.get_source_repo().get_repoid();
    let counter_name = format_counter(&source_repo_id);

    let counter = target_repo_dbs
        .counters
        .get_counter(&ctx, &counter_name)
        .await?
        .unwrap_or(0);

    debug!(ctx.logger(), "fetched counter {}", counter);

    let log_entries_limit = match limit {
        BacksyncLimit::Limit(limit) => limit,
        BacksyncLimit::NoLimit => {
            // Set limit extremely high to read all new values
            u64::max_value()
        }
    };
    let next_entries: Vec<_> = commit_syncer
        .get_source_repo()
        .read_next_bookmark_log_entries(
            ctx.clone(),
            counter as u64,
            log_entries_limit,
            Freshness::MostRecent,
        )
        .try_collect()
        .await?;

    // Before syncing entries, check if cancellation has been
    // requested. If yes, then exit early.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!(ctx.logger(), "sync stopping due to cancellation request");
        return Ok(());
    }

    if next_entries.is_empty() {
        debug!(ctx.logger(), "nothing to sync");
        Ok(())
    } else {
        sync_entries(
            ctx,
            &commit_syncer,
            target_repo_dbs,
            next_entries,
            counter as i64,
            cancellation_requested,
        )
        .await
    }
}

async fn sync_entries<M>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    target_repo_dbs: TargetRepoDbs,
    entries: Vec<BookmarkUpdateLogEntry>,
    mut counter: i64,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    for entry in entries {
        // Before processing each entry, check if cancellation has
        // been requested and exit if that's the case.
        if cancellation_requested.load(Ordering::Relaxed) {
            info!(ctx.logger(), "sync stopping due to cancellation request");
            return Ok(());
        }
        let entry_id = entry.id;
        if counter >= entry_id {
            continue;
        }
        debug!(ctx.logger(), "backsyncing {} ...", entry_id);

        let mut scuba_sample = ctx.scuba().clone();
        scuba_sample.add("backsyncer_bookmark_log_entry_id", entry.id);

        let start_instant = Instant::now();

        if let Some(to_cs_id) = entry.to_changeset_id {
            let (_, unsynced_ancestors_versions) =
                find_toposorted_unsynced_ancestors(&ctx, commit_syncer, to_cs_id).await?;

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
                scuba_sample.log_with_msg(
                    "Skipping entry because there are no synced ancestors",
                    Some(format!("{}", entry.id)),
                );
                target_repo_dbs
                    .counters
                    .set_counter(
                        &ctx,
                        &format_counter(&commit_syncer.get_source_repo().get_repoid()),
                        entry.id,
                        Some(counter),
                    )
                    .await?;
                counter = entry.id;
                continue;
            }

            // Backsyncer is always used in the large-to-small direction,
            // therefore there can be at most one remapped candidate,
            // so `CandidateSelectionHint::Only` is a safe choice
            commit_syncer
                .sync_commit(
                    &ctx,
                    to_cs_id,
                    CandidateSelectionHint::Only,
                    CommitSyncContext::Backsyncer,
                )
                .await?;
        }

        let new_counter = entry.id;
        let success = backsync_bookmark(
            ctx.clone(),
            commit_syncer,
            target_repo_dbs.clone(),
            Some(counter),
            entry,
        )
        .await?;

        scuba_sample.add(
            "backsync_duration_ms",
            u64::try_from(start_instant.elapsed().as_millis()).unwrap_or(u64::max_value()),
        );
        scuba_sample.add("backsync_previously_done", !success);
        scuba_sample.log_with_msg("Backsyncing", None);

        if success {
            counter = new_counter;
        } else {
            debug!(
                ctx.logger(),
                "failed to backsync {}, most likely another process already synced it ", entry_id
            );
            // Transaction failed, it could be because another process already backsynced it
            // Verify that counter was moved and continue if that's the case

            let source_repo_id = commit_syncer.get_source_repo().get_repoid();
            let counter_name = format_counter(&source_repo_id);
            let new_counter = target_repo_dbs
                .counters
                .get_counter(&ctx, &counter_name)
                .await?
                .unwrap_or(0);
            if new_counter <= counter {
                return Err(format_err!(
                    "backsync transaction failed, but the counter didn't move forward. Was {}, became {}",
                    counter,
                    new_counter,
                ));
            } else {
                debug!(
                    ctx.logger(),
                    "verified that another process has already synced {}", entry_id
                );
                counter = new_counter;
            }
        }
    }
    Ok(())
}

async fn backsync_bookmark<M>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    target_repo_dbs: TargetRepoDbs,
    prev_counter: Option<i64>,
    log_entry: BookmarkUpdateLogEntry,
) -> Result<bool, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let target_repo_id = commit_syncer.get_target_repo().get_repoid();
    let source_repo_id = commit_syncer.get_source_repo().get_repoid();

    debug!(ctx.logger(), "preparing to backsync {:?}", log_entry);

    let new_counter = log_entry.id;
    let bookmark = commit_syncer.get_bookmark_renamer().await?(&log_entry.bookmark_name);
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

    let txn_hook = Arc::new({
        move |ctx: CoreContext, txn: Transaction| {
            async move {
                // This is an abstraction leak: it only works because the
                // mutable counters are stored in the same db as the
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

                match txn_result {
                    TransactionResult::Succeeded(txn) => Ok(txn),
                    TransactionResult::Failed => Err(BookmarkTransactionError::LogicError),
                }
            }
            .boxed()
        }
    });

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

        if from_cs_id != to_cs_id {
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

            return bookmark_txn.commit_with_hook(txn_hook).await;
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

    let updated = target_repo_dbs
        .counters
        .set_counter(
            &ctx,
            &format_counter(&source_repo_id),
            new_counter,
            prev_counter,
        )
        .await?;

    Ok(updated)
}

// TODO(stash): T56228235 - consider removing SqlMutableCounters and SqlBookmarks and use static
// methods instead
#[derive(Clone)]
pub struct TargetRepoDbs {
    pub connections: SqlConnections,
    pub bookmarks: ArcBookmarks,
    pub bookmark_update_log: ArcBookmarkUpdateLog,
    pub counters: ArcMutableCounters,
}

pub async fn open_backsyncer_dbs(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    db_config: MetadataDatabaseConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> Result<TargetRepoDbs, Error> {
    let sql_factory =
        make_metadata_sql_factory(ctx.fb, db_config, mysql_options, readonly_storage).await?;

    let connections: SqlConnections = sql_factory
        .make_primary_connections("bookmark_mutable_counters".to_string())
        .await?
        .into();

    Ok(TargetRepoDbs {
        connections,
        bookmarks: blobrepo.bookmarks().clone(),
        bookmark_update_log: blobrepo.bookmark_update_log().clone(),
        counters: blobrepo.mutable_counters_arc(),
    })
}

pub fn format_counter(repo_to_backsync_from: &RepositoryId) -> String {
    format!("backsync_from_{}", repo_to_backsync_from.id())
}
