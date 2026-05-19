/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use context::PerfCounterType;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_ext::Connection;
use sql_ext::Transaction as SqlTransaction;
use sql_ext::mononoke_queries;
use stats::prelude::*;

use crate::store::SelectBookmark;

const MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT: usize = 10;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    bookmarks_update_log_insert_success: timeseries(Rate, Sum),
    bookmarks_update_log_insert_success_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_update_log_insert_retry: timeseries(Rate, Sum),
    bookmarks_insert_retryable_error: timeseries(Rate, Sum),
    bookmarks_insert_retryable_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_logic_error: timeseries(Rate, Sum),
    bookmarks_insert_logic_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_other_error: timeseries(Rate, Sum),
    bookmarks_insert_other_error_attempt_count: timeseries(Rate, Average, Sum),
}

mononoke_queries! {
    write ReplaceBookmarks(
        values: (repo_id: RepositoryId, log_id: Option<u64>, name: BookmarkName, category: BookmarkCategory, changeset_id: ChangesetId)
    ) {
        none,
        "REPLACE INTO bookmarks (repo_id, log_id, name, category, changeset_id) VALUES {values}"
    }

    pub write InsertBookmarks(
        values: (repo_id: RepositoryId, log_id: Option<u64>, name: BookmarkName, category: BookmarkCategory, changeset_id: ChangesetId, kind: BookmarkKind)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmarks (repo_id, log_id, name, category, changeset_id, hg_kind) VALUES {values}"
    }

    write InsertOrUpdateBookmarks(
        values: (repo_id: RepositoryId, log_id: Option<u64>, name: BookmarkName, category: BookmarkCategory, changeset_id: ChangesetId, kind: BookmarkKind)
    ) {
         none,
        mysql("INSERT INTO bookmarks (repo_id, log_id, name, category, changeset_id, hg_kind) VALUES {values} ON DUPLICATE KEY UPDATE changeset_id = VALUES(changeset_id), hg_kind = VALUES(hg_kind)")
        sqlite("INSERT INTO bookmarks (repo_id, log_id, name, category, changeset_id, hg_kind) VALUES {values} ON CONFLICT (repo_id, name, category) DO UPDATE SET changeset_id = EXCLUDED.changeset_id, hg_kind = EXCLUDED.hg_kind")
    }

    pub write UpdateBookmark(
        repo_id: RepositoryId,
        log_id: Option<u64>,
        name: BookmarkName,
        category: BookmarkCategory,
        old_id: ChangesetId,
        new_id: ChangesetId,
        >list kinds: BookmarkKind
    ) {
        none,
        "UPDATE bookmarks
         SET log_id = {log_id}, changeset_id = {new_id}
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
           AND changeset_id = {old_id}
           AND hg_kind IN {kinds}"
    }

    write DeleteBookmark(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}"
    }

    pub write DeleteBookmarkIf(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory, changeset_id: ChangesetId) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
           AND changeset_id = {changeset_id}"
    }

    pub read FindMaxBookmarkLogId(repo_id: RepositoryId) -> (Option<u64>) {
        "SELECT MAX(id) FROM bookmarks_update_log WHERE repo_id = {repo_id}"
    }

    pub write AddBookmarkLog(
        values: (
            id: u64,
            repo_id: RepositoryId,
            name: BookmarkName,
            category: BookmarkCategory,
            from_changeset_id: Option<ChangesetId>,
            to_changeset_id: Option<ChangesetId>,
            reason: BookmarkUpdateReason,
            timestamp: Timestamp,
        ),
    ) {
        none,
        "INSERT INTO bookmarks_update_log
         (id, repo_id, name, category, from_changeset_id, to_changeset_id, reason, timestamp)
         VALUES {values}"
    }

    // Per-bookmark lock acquisition. MySQL uses FOR UPDATE for row-level
    // locking; SQLite relies on its database-level write lock.
    pub read AcquireBookmarkLock(repo_id: RepositoryId, name: BookmarkName) -> (i32) {
        mysql("SELECT 1 FROM bookmark_update_locks WHERE repo_id = {repo_id} AND name = {name} FOR UPDATE")
        sqlite("SELECT 1 FROM bookmark_update_locks WHERE repo_id = {repo_id} AND name = {name}")
    }

    // Insert a lock row if it doesn't exist (graceful fallback for
    // bookmarks that predate the lock table).
    pub write EnsureBookmarkLockRow(values: (repo_id: RepositoryId, name: BookmarkName)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmark_update_locks (repo_id, name) VALUES {values}"
    }

    // Read the current bookmark value within an existing transaction
    // that already holds a FOR UPDATE lock on the bookmark.
    pub read ReadBookmarkUnderLock(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory) -> (ChangesetId) {
        "SELECT changeset_id FROM bookmarks WHERE repo_id = {repo_id} AND name = {name} AND category = {category}"
    }

    // Allocate a globally unique monotonic log ID via auto-increment.
    pub write AllocateBookmarkLogId() {
        none,
        "INSERT INTO bookmark_log_id_sequence VALUES (NULL)"
    }

    // Read the auto-increment ID that was just allocated.
    pub read ReadLastInsertId() -> (u64) {
        mysql("SELECT LAST_INSERT_ID()")
        sqlite("SELECT last_insert_rowid()")
    }

    // Read the global max ID across ALL repos in bookmarks_update_log.
    // Used to seed the sequence table on first transition to the new path.
    pub read FindGlobalMaxBookmarkLogId() -> (Option<u64>) {
        "SELECT MAX(id) FROM bookmarks_update_log"
    }

    // Seed the sequence table with an explicit ID so that subsequent
    // auto-increment allocations start above existing log entries.
    // Uses INSERT OR IGNORE to be idempotent if two concurrent
    // transactions race to seed the same value.
    pub write SeedSequenceId(id: u64) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmark_log_id_sequence (id) VALUES ({id})"
    }

    // Read the current max ID in the sequence table (NULL if empty).
    pub read ReadMaxSequenceId() -> (Option<u64>) {
        "SELECT MAX(id) FROM bookmark_log_id_sequence"
    }
}

struct NewUpdateLogEntry {
    /// The old bookmarked changeset (if known)
    old: Option<ChangesetId>,

    /// The new bookmarked changeset (or None if the bookmark is being
    /// deleted).
    new: Option<ChangesetId>,

    /// The reason for the update.
    reason: BookmarkUpdateReason,
}

impl NewUpdateLogEntry {
    fn new(
        old: Option<ChangesetId>,
        new: Option<ChangesetId>,
        reason: BookmarkUpdateReason,
    ) -> Result<NewUpdateLogEntry> {
        Ok(NewUpdateLogEntry { old, new, reason })
    }
}

struct SqlBookmarksTransactionPayload {
    /// The repository we are updating.
    repo_id: RepositoryId,

    /// Operations to force-set a bookmark to a changeset.
    force_sets: Vec<(BookmarkKey, ChangesetId, NewUpdateLogEntry)>,

    /// Operations to create a bookmark.
    creates: Vec<(
        BookmarkKey,
        ChangesetId,
        BookmarkKind,
        Option<NewUpdateLogEntry>,
    )>,

    /// Operations to create or update a bookmark.
    creates_or_updates: Vec<(
        BookmarkKey,
        ChangesetId,
        BookmarkKind,
        Option<NewUpdateLogEntry>,
    )>,

    /// Operations to update a bookmark from an old id to a new id, provided
    /// it has a matching kind.
    updates: Vec<(
        BookmarkKey,
        ChangesetId,
        ChangesetId,
        &'static [BookmarkKind],
        Option<NewUpdateLogEntry>,
    )>,

    /// Operations to force-delete a bookmark.
    force_deletes: Vec<(BookmarkKey, NewUpdateLogEntry)>,

    /// Operations to delete a bookmark with an old id.
    deletes: Vec<(BookmarkKey, ChangesetId, Option<NewUpdateLogEntry>)>,
}

/// Source of log IDs for a bookmark transaction.
enum LogIdSource {
    /// Old path: sequential IDs starting from MAX(id) + 1.
    Sequential { next_id: u64 },
    /// New path: pre-allocated IDs from the auto-increment sequence table.
    PreAllocated { ids: Vec<u64>, cursor: usize },
}

/// Structure representing the log entries to insert when executing a
/// SqlBookmarksTransactionPayload.
struct TransactionLogUpdates<'a> {
    id_source: LogIdSource,
    log_entries: Vec<(u64, &'a BookmarkKey, &'a NewUpdateLogEntry)>,
}

impl<'a> TransactionLogUpdates<'a> {
    fn sequential(next_log_id: u64) -> Self {
        Self {
            id_source: LogIdSource::Sequential {
                next_id: next_log_id,
            },
            log_entries: Vec::new(),
        }
    }

    fn pre_allocated(ids: Vec<u64>) -> Self {
        Self {
            id_source: LogIdSource::PreAllocated { ids, cursor: 0 },
            log_entries: Vec::new(),
        }
    }

    fn push_log_entry(
        &mut self,
        bookmark: &'a BookmarkKey,
        entry: &'a NewUpdateLogEntry,
    ) -> Result<u64> {
        let id = match &mut self.id_source {
            LogIdSource::Sequential { next_id } => {
                let id = *next_id;
                *next_id += 1;
                id
            }
            LogIdSource::PreAllocated { ids, cursor } => {
                let id = *ids.get(*cursor).ok_or_else(|| {
                    anyhow!(
                        "Pre-allocated ID cursor {} exceeds available IDs ({})",
                        *cursor,
                        ids.len()
                    )
                })?;
                *cursor += 1;
                id
            }
        };
        self.log_entries.push((id, bookmark, entry));
        Ok(id)
    }
}

impl SqlBookmarksTransactionPayload {
    fn new(repo_id: RepositoryId) -> Self {
        SqlBookmarksTransactionPayload {
            repo_id,
            force_sets: Vec::new(),
            creates: Vec::new(),
            creates_or_updates: Vec::new(),
            updates: Vec::new(),
            force_deletes: Vec::new(),
            deletes: Vec::new(),
        }
    }

    fn use_per_bookmark_locking(&self) -> Result<bool> {
        let switch = self.repo_id.id().to_string();
        justknobs::eval("scm/mononoke:per_bookmark_locking", None, Some(&switch)).with_context(
            || {
                format!(
                    "Failed to read per_bookmark_locking JustKnob for repo {}",
                    self.repo_id
                )
            },
        )
    }

    fn use_per_bookmark_locking_shadow_mode(&self) -> Result<bool> {
        let switch = self.repo_id.id().to_string();
        justknobs::eval(
            "scm/mononoke:per_bookmark_locking_shadow",
            None,
            Some(&switch),
        )
        .with_context(|| {
            format!(
                "Failed to read per_bookmark_locking_shadow JustKnob for repo {}",
                self.repo_id
            )
        })
    }

    /// Acquire per-bookmark locks for all bookmarks being modified.
    /// Locks are acquired in sorted order to prevent deadlocks.
    async fn acquire_bookmark_locks(
        &self,
        _ctx: &CoreContext,
        mut txn: SqlTransaction,
    ) -> Result<SqlTransaction> {
        let mut bookmark_names: Vec<&BookmarkName> = Vec::new();
        for (bk, _, _) in &self.force_sets {
            bookmark_names.push(bk.name());
        }
        for (bk, _, _, _) in &self.creates {
            bookmark_names.push(bk.name());
        }
        for (bk, _, _, _) in &self.creates_or_updates {
            bookmark_names.push(bk.name());
        }
        for (bk, _, _, _, _) in &self.updates {
            bookmark_names.push(bk.name());
        }
        for (bk, _) in &self.force_deletes {
            bookmark_names.push(bk.name());
        }
        for (bk, _, _) in &self.deletes {
            bookmark_names.push(bk.name());
        }

        // Sort for deterministic lock ordering (prevents deadlocks).
        // No dedup needed: each bookmark operation type enforces unique bookmarks
        // via the BookmarkTransaction trait, so no bookmark appears twice.
        bookmark_names.sort();

        for name in bookmark_names {
            txn = acquire_single_bookmark_lock(txn, &self.repo_id, name).await?;
        }
        Ok(txn)
    }

    /// Allocate N globally unique log IDs via the auto-increment sequence table.
    async fn allocate_log_ids(
        _ctx: &CoreContext,
        txn: SqlTransaction,
        count: usize,
    ) -> Result<(SqlTransaction, Vec<u64>)> {
        allocate_log_ids_from_sequence(txn, count).await
    }

    /// Count the number of log entries this transaction will produce.
    fn count_log_entries(&self) -> usize {
        self.force_sets.len()
            + self
                .creates
                .iter()
                .filter(|(_, _, _, log)| log.is_some())
                .count()
            + self
                .creates_or_updates
                .iter()
                .filter(|(_, _, _, log)| log.is_some())
                .count()
            + self
                .updates
                .iter()
                .filter(|(_, _, _, _, log)| log.is_some())
                .count()
            + self.force_deletes.len()
            + self
                .deletes
                .iter()
                .filter(|(_, _, log)| log.is_some())
                .count()
    }

    async fn find_next_update_log_id(
        _ctx: &CoreContext,
        txn: SqlTransaction,
        repo_id: RepositoryId,
    ) -> Result<(SqlTransaction, u64)> {
        let (txn, max_id_entries) =
            FindMaxBookmarkLogId::query_with_transaction(txn, &repo_id).await?;

        let next_id = match &max_id_entries[..] {
            [(None,)] => 1,
            [(Some(max_existing),)] => *max_existing + 1,
            _ => {
                return Err(anyhow!(
                    "FindMaxBookmarkLogId returned multiple entries: {:?}",
                    max_id_entries
                ));
            }
        };
        Ok((txn, next_id))
    }

    async fn store_log<'a>(
        &'a self,
        _ctx: &CoreContext,
        mut txn: SqlTransaction,
        log: &'a TransactionLogUpdates<'a>,
    ) -> Result<SqlTransaction> {
        let timestamp = Timestamp::now();

        for (id, bookmark, log_entry) in log.log_entries.iter() {
            let data = [(
                id,
                &self.repo_id,
                bookmark.name(),
                bookmark.category(),
                &log_entry.old,
                &log_entry.new,
                &log_entry.reason,
                &timestamp,
            )];
            txn = AddBookmarkLog::query_with_transaction(txn, &data[..])
                .await?
                .0;
        }
        Ok(txn)
    }

    async fn store_force_sets<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id, log_entry) in self.force_sets.iter() {
            let log_id = log
                .push_log_entry(bookmark, log_entry)
                .map_err(BookmarkTransactionError::RetryableError)?;
            data.push((self.repo_id, Some(log_id), bookmark, cs_id));
        }
        let data = data
            .iter()
            .map(|(repo_id, log_id, bookmark, cs_id)| {
                (
                    repo_id,
                    log_id,
                    bookmark.name(),
                    bookmark.category(),
                    *cs_id,
                )
            })
            .collect::<Vec<_>>();
        let (txn, _) = ReplaceBookmarks::query_with_transaction(txn, data.as_slice()).await?;
        Ok(txn)
    }

    async fn store_creates<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id, kind, maybe_log_entry) in self.creates.iter() {
            let log_id = maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry))
                .transpose()
                .map_err(BookmarkTransactionError::RetryableError)?;
            data.push((self.repo_id, log_id, bookmark, cs_id, kind))
        }
        let data = data
            .iter()
            .map(|(repo_id, log_id, bookmark, cs_id, kind)| {
                (
                    repo_id,
                    log_id,
                    bookmark.name(),
                    bookmark.category(),
                    *cs_id,
                    *kind,
                )
            })
            .collect::<Vec<_>>();
        let rows_to_insert = data.len() as u64;
        let (txn, result) = InsertBookmarks::query_with_transaction(txn, data.as_slice()).await?;
        if result.affected_rows() != rows_to_insert {
            return Err(BookmarkTransactionError::LogicError);
        }
        Ok(txn)
    }

    async fn store_creates_or_updates<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id, kind, maybe_log_entry) in self.creates_or_updates.iter() {
            let log_id = maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry))
                .transpose()
                .map_err(BookmarkTransactionError::RetryableError)?;
            data.push((self.repo_id, log_id, bookmark, cs_id, kind))
        }
        let data = data
            .iter()
            .map(|(repo_id, log_id, bookmark, cs_id, kind)| {
                (
                    repo_id,
                    log_id,
                    bookmark.name(),
                    bookmark.category(),
                    *cs_id,
                    *kind,
                )
            })
            .collect::<Vec<_>>();
        let rows_to_insert = data.len() as u64;
        let (txn, result) =
            InsertOrUpdateBookmarks::query_with_transaction(txn, data.as_slice()).await?;
        if result.affected_rows() < rows_to_insert {
            return Err(BookmarkTransactionError::LogicError);
        }
        Ok(txn)
    }

    async fn store_updates<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, old_cs_id, new_cs_id, kinds, maybe_log_entry) in self.updates.iter() {
            let log_id = maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry))
                .transpose()
                .map_err(BookmarkTransactionError::RetryableError)?;

            if new_cs_id == old_cs_id && log_id.is_none() {
                // This is a no-op update.  Check if the bookmark already points to the correct
                // commit.  If it doesn't, abort the transaction. We need to make this a select
                // query instead of an update, since affected_rows() would otherwise return 0.
                let (txn_, result) = SelectBookmark::query_with_transaction(
                    txn,
                    &self.repo_id,
                    bookmark.name(),
                    bookmark.category(),
                )
                .await?;
                txn = txn_;
                if result.first().map(|row| row.0).as_ref() != Some(new_cs_id) {
                    return Err(BookmarkTransactionError::LogicError);
                }
            } else {
                let (txn_, result) = UpdateBookmark::query_with_transaction(
                    txn,
                    &self.repo_id,
                    &log_id,
                    bookmark.name(),
                    bookmark.category(),
                    old_cs_id,
                    new_cs_id,
                    kinds,
                )
                .await?;
                txn = txn_;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
            }
        }
        Ok(txn)
    }

    async fn store_force_deletes<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, log_entry) in self.force_deletes.iter() {
            log.push_log_entry(bookmark, log_entry)
                .map_err(BookmarkTransactionError::RetryableError)?;
            let (txn_, _) = DeleteBookmark::query_with_transaction(
                txn,
                &self.repo_id,
                bookmark.name(),
                bookmark.category(),
            )
            .await?;
            txn = txn_;
        }
        Ok(txn)
    }

    async fn store_deletes<'op, 'log: 'op>(
        &'log self,
        _ctx: &CoreContext,
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, old_cs_id, maybe_log_entry) in self.deletes.iter() {
            maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry))
                .transpose()
                .map_err(BookmarkTransactionError::RetryableError)?;
            let (txn_, result) = DeleteBookmarkIf::query_with_transaction(
                txn,
                &self.repo_id,
                bookmark.name(),
                bookmark.category(),
                old_cs_id,
            )
            .await?;
            txn = txn_;
            if result.affected_rows() != 1 {
                return Err(BookmarkTransactionError::LogicError);
            }
        }
        Ok(txn)
    }

    /// Attempt to write a bookmark update log entry
    /// Returns the db transaction and the id of this entry in the bookmark update log.
    async fn attempt_write(
        &self,
        ctx: &CoreContext,
        txn: SqlTransaction,
    ) -> Result<(SqlTransaction, u64), BookmarkTransactionError> {
        // JK errors are hard errors (Other), not retryable: a missing JustKnob
        // won't fix itself on retry — it requires config/deployment action.
        let use_new_path = self
            .use_per_bookmark_locking()
            .map_err(BookmarkTransactionError::Other)?;
        let shadow_mode = !use_new_path
            && self
                .use_per_bookmark_locking_shadow_mode()
                .map_err(BookmarkTransactionError::Other)?;

        if shadow_mode {
            // Shadow mode: log what per-bookmark locking would do, without
            // changing the transaction path. We can't run both paths in the
            // same SQL transaction because the old path's SELECT MAX(id)
            // would still take the gap lock.
            let bookmark_names: Vec<String> = self
                .force_sets
                .iter()
                .map(|(bk, _, _)| bk.name())
                .chain(self.creates.iter().map(|(bk, _, _, _)| bk.name()))
                .chain(
                    self.creates_or_updates
                        .iter()
                        .map(|(bk, _, _, _)| bk.name()),
                )
                .chain(self.updates.iter().map(|(bk, _, _, _, _)| bk.name()))
                .chain(self.force_deletes.iter().map(|(bk, _)| bk.name()))
                .chain(self.deletes.iter().map(|(bk, _, _)| bk.name()))
                .map(|n| n.to_string())
                .collect();
            ctx.scuba()
                .clone()
                .add(
                    "per_bookmark_lock_bookmark_count",
                    bookmark_names.len() as i64,
                )
                .add("per_bookmark_lock_repo_id", self.repo_id.id())
                .add("per_bookmark_lock_bookmarks", bookmark_names)
                .log_with_msg("per_bookmark_locking_shadow", None);
        }

        let (mut txn, mut log) = if use_new_path {
            // New path: per-bookmark locks + auto-increment IDs
            let new_path_start = std::time::Instant::now();
            let txn = self
                .acquire_bookmark_locks(ctx, txn)
                .await
                .map_err(BookmarkTransactionError::RetryableError)?;
            let lock_acquired_us = new_path_start.elapsed().as_micros() as i64;
            let log_entry_count = self.count_log_entries();
            let (txn, ids) = Self::allocate_log_ids(ctx, txn, log_entry_count)
                .await
                .map_err(BookmarkTransactionError::RetryableError)?;
            let id_alloc_us = new_path_start.elapsed().as_micros() as i64 - lock_acquired_us;

            // Unsampled telemetry: one row per bookmark-write on repos that have
            // flipped per_bookmark_locking on. Lets us see lock-acquisition and
            // ID-allocation latency distributions in production. Volume scales
            // with bookmark-write rate; expected to be modest for current
            // Phase 3 targets and revisitable if rolled out to fbsource master.
            ctx.scuba()
                .clone()
                .unsampled()
                .add("per_bookmark_lock_acquired_us", lock_acquired_us)
                .add("per_bookmark_log_ids_allocated_us", id_alloc_us)
                .add("per_bookmark_lock_repo_id", self.repo_id.id())
                .add("per_bookmark_lock_entry_count", log_entry_count as i64)
                .log_with_msg("per_bookmark_locking_active", None);

            (txn, TransactionLogUpdates::pre_allocated(ids))
        } else {
            // Old path: optimistic locking via repo-level SELECT MAX(id)
            let (txn, next_id) = Self::find_next_update_log_id(ctx, txn, self.repo_id).await?;
            (txn, TransactionLogUpdates::sequential(next_id))
        };

        txn = self.store_force_sets(ctx, txn, &mut log).await?;
        txn = self.store_creates(ctx, txn, &mut log).await?;
        txn = self.store_creates_or_updates(ctx, txn, &mut log).await?;
        txn = self.store_updates(ctx, txn, &mut log).await?;
        txn = self.store_force_deletes(ctx, txn, &mut log).await?;
        txn = self.store_deletes(ctx, txn, &mut log).await?;
        txn = self
            .store_log(ctx, txn, &log)
            .await
            .map_err(BookmarkTransactionError::RetryableError)?;

        // Return the first log ID (used by callers for ensure_backsynced)
        let first_id = log.log_entries.first().map(|(id, _, _)| *id).unwrap_or(0);
        Ok((txn, first_id))
    }
}

pub struct SqlBookmarksTransaction {
    write_connection: Connection,
    ctx: CoreContext,

    /// Bookmarks that have been seen already in this transaction.
    seen: HashSet<BookmarkKey>,

    /// Transaction updates.  A separate struct so that they can be
    /// moved into the future that will perform the database
    /// updates.
    payload: SqlBookmarksTransactionPayload,
}

impl SqlBookmarksTransaction {
    pub(crate) fn new(
        ctx: CoreContext,
        write_connection: Connection,
        repo_id: RepositoryId,
    ) -> Self {
        Self {
            write_connection,
            ctx,
            seen: HashSet::new(),
            payload: SqlBookmarksTransactionPayload::new(repo_id),
        }
    }

    pub fn check_not_seen(&mut self, bookmark: &BookmarkKey) -> Result<()> {
        if !self.seen.insert(bookmark.clone()) {
            return Err(anyhow!("{} bookmark was already used", bookmark));
        }
        Ok(())
    }
}

impl BookmarkTransaction for SqlBookmarksTransaction {
    fn update(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(Some(old_cs), Some(new_cs), reason)?;
        self.payload.updates.push((
            bookmark.clone(),
            old_cs,
            new_cs,
            BookmarkKind::ALL_PUBLISHING,
            Some(log),
        ));
        Ok(())
    }

    fn update_scratch(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.updates.push((
            bookmark.clone(),
            old_cs,
            new_cs,
            &[BookmarkKind::Scratch],
            None, // Scratch bookmark updates are not logged.
        ));
        Ok(())
    }

    fn create(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, Some(new_cs), reason)?;

        self.payload.creates.push((
            bookmark.clone(),
            new_cs,
            BookmarkKind::PullDefaultPublishing,
            Some(log),
        ));

        Ok(())
    }

    fn creates_or_updates(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, Some(new_cs), reason)?;

        self.payload.creates_or_updates.push((
            bookmark.clone(),
            new_cs,
            BookmarkKind::PullDefaultPublishing,
            Some(log),
        ));

        Ok(())
    }

    fn create_publishing(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, Some(new_cs), reason)?;
        self.payload.creates.push((
            bookmark.clone(),
            new_cs,
            BookmarkKind::Publishing,
            Some(log),
        ));
        Ok(())
    }

    fn create_scratch(&mut self, bookmark: &BookmarkKey, new_cs: ChangesetId) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.creates.push((
            bookmark.clone(),
            new_cs,
            BookmarkKind::Scratch,
            None, // Scratch bookmark updates are not logged.
        ));
        Ok(())
    }

    fn force_set(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, Some(new_cs), reason)?;
        self.payload
            .force_sets
            .push((bookmark.clone(), new_cs, log));
        Ok(())
    }

    fn delete(
        &mut self,
        bookmark: &BookmarkKey,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(Some(old_cs), None, reason)?;
        self.payload
            .deletes
            .push((bookmark.clone(), old_cs, Some(log)));
        Ok(())
    }

    fn force_delete(&mut self, bookmark: &BookmarkKey, reason: BookmarkUpdateReason) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, None, reason)?;
        self.payload.force_deletes.push((bookmark.clone(), log));
        Ok(())
    }

    fn delete_scratch(&mut self, bookmark: &BookmarkKey, old_cs: ChangesetId) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.deletes.push((
            bookmark.clone(),
            old_cs,
            None, // Scratch bookmark updates are not logged.
        ));
        Ok(())
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<Option<u64>>> {
        self.commit_with_hooks(vec![Arc::new(|_ctx, txn| future::ok(txn).boxed())])
    }

    fn commit_with_hooks(
        self: Box<Self>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> BoxFuture<'static, Result<Option<u64>>> {
        let Self {
            ctx,
            payload,
            write_connection,
            ..
        } = *self;

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        async move {
            let mut attempt = 0;
            let result: Result<(sql_ext::Transaction, u64), _> = loop {
                attempt += 1;

                let mut txn = write_connection
                    .start_transaction(ctx.sql_query_telemetry())
                    .await?;

                txn = match run_transaction_hooks(&ctx, txn, &txn_hooks).await {
                    Ok(txn) => txn,
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue;
                    }
                    Err(err) => break Err(err),
                };

                match payload.attempt_write(&ctx, txn).await {
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue;
                    }
                    result => break result,
                }
            };

            // The number of `RetryableError`'s that were encountered
            let mut retryable_errors = attempt as i64 - 1;
            let result = match result {
                Ok((txn, log_id)) => {
                    STATS::bookmarks_update_log_insert_success.add_value(1);
                    STATS::bookmarks_update_log_insert_success_attempt_count
                        .add_value(attempt as i64);
                    txn.commit().await?;
                    Ok(Some(log_id))
                }
                Err(BookmarkTransactionError::LogicError) => {
                    // Logic error signifies that the transaction was rolled
                    // back, which likely means that bookmark has moved since
                    // our pushrebase finished. We need to retry the pushrebase
                    // Attempt count means one more than the number of `RetryableError`
                    // we hit before seeing this.
                    STATS::bookmarks_insert_logic_error.add_value(1);
                    STATS::bookmarks_insert_logic_error_attempt_count.add_value(attempt as i64);
                    Ok(None)
                }
                Err(BookmarkTransactionError::RetryableError(err)) => {
                    // Attempt count for `RetryableError` should always be equal
                    // to the MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT, and hitting
                    // this error here basically means that this number of attempts
                    // was not enough, or the error was misclassified
                    STATS::bookmarks_insert_retryable_error.add_value(1);
                    STATS::bookmarks_insert_retryable_error_attempt_count.add_value(attempt as i64);
                    retryable_errors += 1;
                    Err(err)
                }
                Err(BookmarkTransactionError::Other(err)) => {
                    // `Other` error captures what we consider an "infrastructure"
                    // error, e.g. xdb went down during this transaction.
                    // Attempt count > 1 means the before we hit this error,
                    // we hit `RetryableError` a attempt count - 1 times.
                    STATS::bookmarks_insert_other_error.add_value(1);
                    STATS::bookmarks_insert_other_error_attempt_count.add_value(attempt as i64);
                    Err(err)
                }
            };
            STATS::bookmarks_update_log_insert_retry.add_value(retryable_errors);
            result
        }
        .boxed()
    }
}

async fn run_transaction_hooks(
    ctx: &CoreContext,
    mut txn: sql_ext::Transaction,
    txn_hooks: &Vec<BookmarkTransactionHook>,
) -> Result<sql_ext::Transaction, BookmarkTransactionError> {
    for txn_hook in txn_hooks {
        txn = txn_hook(ctx.clone(), txn).await?;
    }
    Ok(txn)
}

#[cfg(test)]
pub(crate) async fn insert_bookmarks(
    ctx: &CoreContext,
    conn: &Connection,
    rows: impl IntoIterator<Item = (&RepositoryId, &BookmarkKey, &ChangesetId, &BookmarkKind)>,
) -> Result<()> {
    let none = None;
    let rows = rows
        .into_iter()
        .map(|(r, b, c, k)| (r, &none, b.name(), b.category(), c, k))
        .collect::<Vec<_>>();
    InsertBookmarks::query(conn, ctx.sql_query_telemetry(), rows.as_slice()).await?;
    Ok(())
}

/// Acquire a per-bookmark FOR UPDATE lock on a single bookmark.
///
/// If the lock row doesn't exist (legacy bookmark predating the lock table),
/// creates it via INSERT IGNORE and re-acquires. This graceful fallback
/// eliminates the need for a coordinated backfill migration.
///
/// Used by both `SqlBookmarksTransactionPayload::acquire_bookmark_locks`
/// (per-bookmark locking optimistic path) and `LockedBookmarkTransaction::new`
/// (pessimistic path).
pub(crate) async fn acquire_single_bookmark_lock(
    txn: SqlTransaction,
    repo_id: &RepositoryId,
    name: &BookmarkName,
) -> Result<SqlTransaction> {
    let (txn, rows) = AcquireBookmarkLock::query_with_transaction(txn, repo_id, name).await?;

    if rows.is_empty() {
        // Lock row doesn't exist (legacy bookmark). Create it and re-acquire.
        let data = [(repo_id, name)];
        let (txn, _) = EnsureBookmarkLockRow::query_with_transaction(txn, &data[..]).await?;
        let (txn, _) = AcquireBookmarkLock::query_with_transaction(txn, repo_id, name).await?;
        Ok(txn)
    } else {
        Ok(txn)
    }
}

/// Allocate N globally unique log IDs via the auto-increment sequence table.
///
/// On first use (empty sequence table), seeds the table from the global
/// MAX(id) in bookmarks_update_log so that new IDs don't conflict with
/// existing log entries. This makes the old→new→old→new transition safe.
///
/// Used by both `SqlBookmarksTransactionPayload::allocate_log_ids` (per-bookmark
/// locking optimistic path) and `LockedBookmarkTransaction::commit` (pessimistic
/// path).
pub(crate) async fn allocate_log_ids_from_sequence(
    mut txn: SqlTransaction,
    count: usize,
) -> Result<(SqlTransaction, Vec<u64>)> {
    if count == 0 {
        return Ok((txn, vec![]));
    }

    // Seed the sequence table on first use so new IDs start above
    // existing entries in bookmarks_update_log.
    let (txn_, rows) = ReadMaxSequenceId::query_with_transaction(txn).await?;
    txn = txn_;
    if rows.first().and_then(|r| r.0).is_none() {
        let (txn_, global_max_rows) =
            FindGlobalMaxBookmarkLogId::query_with_transaction(txn).await?;
        txn = txn_;
        if let Some(max_id) = global_max_rows.first().and_then(|r| r.0) {
            let (txn_, _) = SeedSequenceId::query_with_transaction(txn, &max_id).await?;
            txn = txn_;
        }
    }

    // Allocate N IDs by inserting N individual rows, reading back each
    // ID immediately after its INSERT via LAST_INSERT_ID(). We must read
    // after each INSERT because MySQL auto-increment is NOT transactional:
    // the AUTO-INC lock is per-statement (not per-transaction), so another
    // connection can allocate an ID between any two of our INSERTs,
    // creating gaps. Reading LAST_INSERT_ID() after each INSERT is safe
    // because it is per-connection — it always returns the last auto-
    // increment value generated by THIS connection regardless of other
    // connections' activity.
    //
    // Performance: N is typically 1 (single-bookmark transactions); the
    // rare multi-bookmark case (e.g., git import) has N < 100.
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        let (txn_, _) = AllocateBookmarkLogId::query_with_transaction(txn).await?;
        let (txn_, rows) = ReadLastInsertId::query_with_transaction(txn_).await?;
        txn = txn_;
        let id = rows
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("ReadLastInsertId returned no rows"))?
            .0;
        ids.push(id);
    }
    Ok((txn, ids))
}
