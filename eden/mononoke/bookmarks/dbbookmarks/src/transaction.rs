/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, Result};
use bookmarks::{
    BookmarkKind, BookmarkName, BookmarkTransaction, BookmarkTransactionError,
    BookmarkTransactionHook, BookmarkUpdateReason, BundleReplay, RawBundleReplayData,
};
use context::{CoreContext, PerfCounterType};
use futures::compat::Future01CompatExt;
use futures::future::{self, BoxFuture, FutureExt};
use mononoke_types::Timestamp;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection, Transaction as SqlTransaction};
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::store::SelectBookmark;

const MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT: usize = 5;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    bookmarks_update_log_insert_success: timeseries(Rate, Sum),
    bookmarks_update_log_insert_success_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_retryable_error: timeseries(Rate, Sum),
    bookmarks_insert_retryable_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_logic_error: timeseries(Rate, Sum),
    bookmarks_insert_logic_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_other_error: timeseries(Rate, Sum),
    bookmarks_insert_other_error_attempt_count: timeseries(Rate, Average, Sum),
}

queries! {
    write ReplaceBookmarks(
        values: (repo_id: RepositoryId, name: BookmarkName, changeset_id: ChangesetId)
    ) {
        none,
        "REPLACE INTO bookmarks (repo_id, name, changeset_id) VALUES {values}"
    }

    write InsertBookmarks(
        values: (repo_id: RepositoryId, name: BookmarkName, changeset_id: ChangesetId, kind: BookmarkKind)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmarks (repo_id, name, changeset_id, hg_kind) VALUES {values}"
    }

    write UpdateBookmark(
        repo_id: RepositoryId,
        name: BookmarkName,
        old_id: ChangesetId,
        new_id: ChangesetId,
        >list kinds: BookmarkKind
    ) {
        none,
        "UPDATE bookmarks
         SET changeset_id = {new_id}
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND changeset_id = {old_id}
           AND hg_kind IN {kinds}"
    }

    write DeleteBookmark(repo_id: RepositoryId, name: BookmarkName) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}"
    }

    write DeleteBookmarkIf(repo_id: RepositoryId, name: BookmarkName, changeset_id: ChangesetId) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND changeset_id = {changeset_id}"
    }

    read FindMaxBookmarkLogId() -> (Option<u64>) {
        "SELECT MAX(id) FROM bookmarks_update_log"
    }

    write AddBookmarkLog(
        values: (
            id: u64,
            repo_id: RepositoryId,
            name: BookmarkName,
            from_changeset_id: Option<ChangesetId>,
            to_changeset_id: Option<ChangesetId>,
            reason: BookmarkUpdateReason,
            timestamp: Timestamp,
        ),
    ) {
        none,
        "INSERT INTO bookmarks_update_log
         (id, repo_id, name, from_changeset_id, to_changeset_id, reason, timestamp)
         VALUES {values}"
    }

    write AddBundleReplayData(values: (id: u64, bundle_handle: String, commit_hashes_json: String)) {
        none,
        "INSERT INTO bundle_replay_data
         (bookmark_update_log_id, bundle_handle, commit_hashes_json)
         VALUES {values}"
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

    /// Bundle replay information if this update is replayable.
    bundle_replay_data: Option<RawBundleReplayData>,
}

impl NewUpdateLogEntry {
    fn new(
        old: Option<ChangesetId>,
        new: Option<ChangesetId>,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<NewUpdateLogEntry> {
        Ok(NewUpdateLogEntry {
            old,
            new,
            reason,
            bundle_replay_data: bundle_replay.map(BundleReplay::to_raw).transpose()?,
        })
    }
}

struct SqlBookmarksTransactionPayload {
    /// The repository we are updating.
    repo_id: RepositoryId,

    /// Operations to force-set a bookmark to a changeset.
    force_sets: HashMap<BookmarkName, ChangesetId>,

    /// Operations to create a bookmark.
    creates: HashMap<BookmarkName, (ChangesetId, BookmarkKind)>,

    /// Operations to update a bookmark from an old id to a new id, provided
    /// it has a matching kind.
    updates: HashMap<BookmarkName, (ChangesetId, ChangesetId, &'static [BookmarkKind])>,

    /// Operations to force-delete a bookmark.
    force_deletes: HashSet<BookmarkName>,

    /// Operations to delete a bookmark with an old id.
    deletes: HashMap<BookmarkName, ChangesetId>,

    /// Log entries to log. Scratch updates and creates are not included in
    /// the log.
    log: HashMap<BookmarkName, NewUpdateLogEntry>,
}

impl SqlBookmarksTransactionPayload {
    fn new(repo_id: RepositoryId) -> Self {
        SqlBookmarksTransactionPayload {
            repo_id,
            force_sets: HashMap::new(),
            creates: HashMap::new(),
            updates: HashMap::new(),
            force_deletes: HashSet::new(),
            deletes: HashMap::new(),
            log: HashMap::new(),
        }
    }

    async fn find_next_update_log_id(txn: SqlTransaction) -> Result<(SqlTransaction, u64)> {
        let (txn, max_id_entries) = FindMaxBookmarkLogId::query_with_transaction(txn)
            .compat()
            .await?;

        let next_id = match &max_id_entries[..] {
            [(None,)] => 1,
            [(Some(max_existing),)] => *max_existing + 1,
            _ => {
                return Err(anyhow!(
                    "FindMaxBookmarkLogId returned multiple entries: {:?}",
                    max_id_entries
                ))
            }
        };
        Ok((txn, next_id))
    }

    async fn store_log(&self, txn: SqlTransaction) -> Result<SqlTransaction> {
        let timestamp = Timestamp::now();
        let (mut txn, mut next_id) = Self::find_next_update_log_id(txn).await?;
        for (bookmark, log_entry) in self.log.iter() {
            let data = [(
                &next_id,
                &self.repo_id,
                bookmark,
                &log_entry.old,
                &log_entry.new,
                &log_entry.reason,
                &timestamp,
            )];
            txn = AddBookmarkLog::query_with_transaction(txn, &data[..])
                .compat()
                .await?
                .0;
            if let Some(data) = &log_entry.bundle_replay_data {
                txn = AddBundleReplayData::query_with_transaction(
                    txn,
                    &[(&next_id, &data.bundle_handle, &data.commit_timestamps_json)],
                )
                .compat()
                .await?
                .0;
            }
            next_id += 1;
        }
        Ok(txn)
    }

    async fn store_force_sets(
        &self,
        txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id) in self.force_sets.iter() {
            data.push((&self.repo_id, bookmark, cs_id));
        }
        let (txn, _) = ReplaceBookmarks::query_with_transaction(txn, data.as_slice())
            .compat()
            .await?;
        Ok(txn)
    }

    async fn store_creates(
        &self,
        txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, &(ref cs_id, ref kind)) in self.creates.iter() {
            data.push((&self.repo_id, bookmark, cs_id, kind))
        }
        let rows_to_insert = data.len() as u64;
        let (txn, result) = InsertBookmarks::query_with_transaction(txn, data.as_slice())
            .compat()
            .await?;
        if result.affected_rows() != rows_to_insert {
            return Err(BookmarkTransactionError::LogicError);
        }
        Ok(txn)
    }

    async fn store_updates(
        &self,
        mut txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, &(ref old_cs_id, ref new_cs_id, kinds)) in self.updates.iter() {
            if new_cs_id == old_cs_id {
                // This is a no-op update.  Check if the bookmark already
                // points to the correct commit.  If it doesn't, abort the
                // transaction.
                let (txn_, result) =
                    SelectBookmark::query_with_transaction(txn, &self.repo_id, bookmark)
                        .compat()
                        .await?;
                txn = txn_;
                if result.get(0).map(|row| row.0).as_ref() != Some(new_cs_id) {
                    return Err(BookmarkTransactionError::LogicError);
                }
            } else {
                let (txn_, result) = UpdateBookmark::query_with_transaction(
                    txn,
                    &self.repo_id,
                    bookmark,
                    old_cs_id,
                    new_cs_id,
                    kinds,
                )
                .compat()
                .await?;
                txn = txn_;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
            }
        }
        Ok(txn)
    }

    async fn store_force_deletes(
        &self,
        mut txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for bookmark in self.force_deletes.iter() {
            let (txn_, _) = DeleteBookmark::query_with_transaction(txn, &self.repo_id, &bookmark)
                .compat()
                .await?;
            txn = txn_;
        }
        Ok(txn)
    }

    async fn store_deletes(
        &self,
        mut txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, old_cs_id) in self.deletes.iter() {
            let (txn_, result) =
                DeleteBookmarkIf::query_with_transaction(txn, &self.repo_id, bookmark, old_cs_id)
                    .compat()
                    .await?;
            txn = txn_;
            if result.affected_rows() != 1 {
                return Err(BookmarkTransactionError::LogicError);
            }
        }
        Ok(txn)
    }

    async fn attempt_write(
        &self,
        mut txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        txn = self.store_force_sets(txn).await?;
        txn = self.store_creates(txn).await?;
        txn = self.store_updates(txn).await?;
        txn = self.store_force_deletes(txn).await?;
        txn = self.store_deletes(txn).await?;
        txn = self
            .store_log(txn)
            .await
            .map_err(BookmarkTransactionError::RetryableError)?;
        Ok(txn)
    }
}

pub struct SqlBookmarksTransaction {
    write_connection: Connection,
    ctx: CoreContext,

    /// Bookmarks that have been seen already in this transaction.
    seen: HashSet<BookmarkName>,

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

    pub fn check_not_seen(&mut self, bookmark: &BookmarkName) -> Result<()> {
        if !self.seen.insert(bookmark.clone()) {
            return Err(anyhow!("{} bookmark was already used", bookmark));
        }
        Ok(())
    }
}

impl BookmarkTransaction for SqlBookmarksTransaction {
    fn update(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.updates.insert(
            bookmark.clone(),
            (old_cs, new_cs, BookmarkKind::ALL_PUBLISHING),
        );
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(Some(old_cs), Some(new_cs), reason, bundle_replay)?,
        );
        Ok(())
    }

    fn create(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.creates.insert(
            bookmark.clone(),
            (new_cs, BookmarkKind::PullDefaultPublishing),
        );
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, Some(new_cs), reason, bundle_replay)?,
        );
        Ok(())
    }

    fn force_set(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.force_sets.insert(bookmark.clone(), new_cs);
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, Some(new_cs), reason, bundle_replay)?,
        );
        Ok(())
    }

    fn delete(
        &mut self,
        bookmark: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.deletes.insert(bookmark.clone(), old_cs);
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(Some(old_cs), None, reason, bundle_replay)?,
        );
        Ok(())
    }

    fn force_delete(
        &mut self,
        bookmark: &BookmarkName,
        reason: BookmarkUpdateReason,
        bundle_replay: Option<&dyn BundleReplay>,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.force_deletes.insert(bookmark.clone());
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, None, reason, bundle_replay)?,
        );
        Ok(())
    }

    fn update_scratch(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload
            .updates
            .insert(bookmark.clone(), (old_cs, new_cs, &[BookmarkKind::Scratch]));
        // Scratch bookmark updates are not logged.
        Ok(())
    }

    fn create_scratch(&mut self, bookmark: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload
            .creates
            .insert(bookmark.clone(), (new_cs, BookmarkKind::Scratch));
        // Scratch bookmark updates are not logged.
        Ok(())
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<bool>> {
        self.commit_with_hook(Arc::new(|_ctx, txn| future::ok(txn).boxed()))
    }

    /// commit_with_hook() can be used to have the same transaction to update two different database
    /// tables. `txn_hook()` should apply changes to the transaction.
    fn commit_with_hook(
        self: Box<Self>,
        txn_hook: BookmarkTransactionHook,
    ) -> BoxFuture<'static, Result<bool>> {
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
            let result = loop {
                attempt += 1;

                let mut txn = write_connection.start_transaction().compat().await?;

                txn = match txn_hook(ctx.clone(), txn).await {
                    Ok(txn) => txn,
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue
                    }
                    err => break err,
                };

                match payload.attempt_write(txn).await {
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue
                    }
                    result => break result,
                }
            };

            match result {
                Ok(txn) => {
                    STATS::bookmarks_update_log_insert_success.add_value(1);
                    STATS::bookmarks_update_log_insert_success_attempt_count
                        .add_value(attempt as i64);
                    txn.commit().compat().await?;
                    Ok(true)
                }
                Err(BookmarkTransactionError::LogicError) => {
                    // Logic error signifies that the transaction was rolled
                    // back, which likely means that bookmark has moved since
                    // our pushrebase finished. We need to retry the pushrebase
                    // Attempt count means one more than the number of `RetryableError`
                    // we hit before seeing this.
                    STATS::bookmarks_insert_logic_error.add_value(1);
                    STATS::bookmarks_insert_logic_error_attempt_count.add_value(attempt as i64);
                    Ok(false)
                }
                Err(BookmarkTransactionError::RetryableError(err)) => {
                    // Attempt count for `RetryableError` should always be equal
                    // to the MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT, and hitting
                    // this error here basically means that this number of attempts
                    // was not enough, or the error was misclassified
                    STATS::bookmarks_insert_retryable_error.add_value(1);
                    STATS::bookmarks_insert_retryable_error_attempt_count.add_value(attempt as i64);
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
            }
        }
        .boxed()
    }
}

#[cfg(test)]
pub(crate) async fn insert_bookmarks(
    conn: &Connection,
    rows: &[(&RepositoryId, &BookmarkName, &ChangesetId, &BookmarkKind)],
) -> Result<()> {
    InsertBookmarks::query(conn, rows).compat().await?;
    Ok(())
}
