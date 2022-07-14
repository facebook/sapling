/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
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
use sql::queries;
use sql::Connection;
use sql::Transaction as SqlTransaction;
use stats::prelude::*;
use std::collections::HashSet;
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
        values: (repo_id: RepositoryId, log_id: Option<u64>, name: BookmarkName, changeset_id: ChangesetId)
    ) {
        none,
        "REPLACE INTO bookmarks (repo_id, log_id, name, changeset_id) VALUES {values}"
    }

    write InsertBookmarks(
        values: (repo_id: RepositoryId, log_id: Option<u64>, name: BookmarkName, changeset_id: ChangesetId, kind: BookmarkKind)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmarks (repo_id, log_id, name, changeset_id, hg_kind) VALUES {values}"
    }

    write UpdateBookmark(
        repo_id: RepositoryId,
        log_id: Option<u64>,
        name: BookmarkName,
        old_id: ChangesetId,
        new_id: ChangesetId,
        >list kinds: BookmarkKind
    ) {
        none,
        "UPDATE bookmarks
         SET log_id = {log_id}, changeset_id = {new_id}
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
    force_sets: Vec<(BookmarkName, ChangesetId, NewUpdateLogEntry)>,

    /// Operations to create a bookmark.
    creates: Vec<(
        BookmarkName,
        ChangesetId,
        BookmarkKind,
        Option<NewUpdateLogEntry>,
    )>,

    /// Operations to update a bookmark from an old id to a new id, provided
    /// it has a matching kind.
    updates: Vec<(
        BookmarkName,
        ChangesetId,
        ChangesetId,
        &'static [BookmarkKind],
        Option<NewUpdateLogEntry>,
    )>,

    /// Operations to force-delete a bookmark.
    force_deletes: Vec<(BookmarkName, NewUpdateLogEntry)>,

    /// Operations to delete a bookmark with an old id.
    deletes: Vec<(BookmarkName, ChangesetId, Option<NewUpdateLogEntry>)>,
}

/// Structure representing the log entries to insert when executing a
/// SqlBookmarksTransactionPayload.
struct TransactionLogUpdates<'a> {
    next_log_id: u64,
    log_entries: Vec<(u64, &'a BookmarkName, &'a NewUpdateLogEntry)>,
}

impl<'a> TransactionLogUpdates<'a> {
    fn new(next_log_id: u64) -> Self {
        Self {
            next_log_id,
            log_entries: Vec::new(),
        }
    }

    fn push_log_entry(&mut self, bookmark: &'a BookmarkName, entry: &'a NewUpdateLogEntry) -> u64 {
        let id = self.next_log_id;
        self.log_entries.push((id, bookmark, entry));
        self.next_log_id += 1;
        id
    }
}

impl SqlBookmarksTransactionPayload {
    fn new(repo_id: RepositoryId) -> Self {
        SqlBookmarksTransactionPayload {
            repo_id,
            force_sets: Vec::new(),
            creates: Vec::new(),
            updates: Vec::new(),
            force_deletes: Vec::new(),
            deletes: Vec::new(),
        }
    }

    async fn find_next_update_log_id(txn: SqlTransaction) -> Result<(SqlTransaction, u64)> {
        let (txn, max_id_entries) = FindMaxBookmarkLogId::query_with_transaction(txn).await?;

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
        mut txn: SqlTransaction,
        log: &'a TransactionLogUpdates<'a>,
    ) -> Result<SqlTransaction> {
        let timestamp = Timestamp::now();

        for (id, bookmark, log_entry) in log.log_entries.iter() {
            let data = [(
                id,
                &self.repo_id,
                *bookmark,
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
        txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id, log_entry) in self.force_sets.iter() {
            let log_id = log.push_log_entry(bookmark, log_entry);
            data.push((self.repo_id, Some(log_id), bookmark, cs_id));
        }
        let data = data
            .iter()
            .map(|(repo_id, log_id, bookmark, cs_id)| (repo_id, log_id, *bookmark, *cs_id))
            .collect::<Vec<_>>();
        let (txn, _) = ReplaceBookmarks::query_with_transaction(txn, data.as_slice()).await?;
        Ok(txn)
    }

    async fn store_creates<'op, 'log: 'op>(
        &'log self,
        txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let mut data = Vec::new();
        for (bookmark, cs_id, kind, maybe_log_entry) in self.creates.iter() {
            let log_id = maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry));
            data.push((self.repo_id, log_id, bookmark, cs_id, kind))
        }
        let data = data
            .iter()
            .map(|(repo_id, log_id, bookmark, cs_id, kind)| {
                (repo_id, log_id, *bookmark, *cs_id, *kind)
            })
            .collect::<Vec<_>>();
        let rows_to_insert = data.len() as u64;
        let (txn, result) = InsertBookmarks::query_with_transaction(txn, data.as_slice()).await?;
        if result.affected_rows() != rows_to_insert {
            return Err(BookmarkTransactionError::LogicError);
        }
        Ok(txn)
    }

    async fn store_updates<'op, 'log: 'op>(
        &'log self,
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, old_cs_id, new_cs_id, kinds, maybe_log_entry) in self.updates.iter() {
            let log_id = maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry));

            if new_cs_id == old_cs_id && log_id.is_none() {
                // This is a no-op update.  Check if the bookmark already points to the correct
                // commit.  If it doesn't, abort the transaction. We need to make this a select
                // query instead of an update, since affected_rows() woud otherwise return 0.
                let (txn_, result) =
                    SelectBookmark::query_with_transaction(txn, &self.repo_id, bookmark).await?;
                txn = txn_;
                if result.get(0).map(|row| row.0).as_ref() != Some(new_cs_id) {
                    return Err(BookmarkTransactionError::LogicError);
                }
            } else {
                let (txn_, result) = UpdateBookmark::query_with_transaction(
                    txn,
                    &self.repo_id,
                    &log_id,
                    bookmark,
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
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, log_entry) in self.force_deletes.iter() {
            log.push_log_entry(bookmark, log_entry);
            let (txn_, _) =
                DeleteBookmark::query_with_transaction(txn, &self.repo_id, bookmark).await?;
            txn = txn_;
        }
        Ok(txn)
    }

    async fn store_deletes<'op, 'log: 'op>(
        &'log self,
        mut txn: SqlTransaction,
        log: &'op mut TransactionLogUpdates<'log>,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        for (bookmark, old_cs_id, maybe_log_entry) in self.deletes.iter() {
            maybe_log_entry
                .as_ref()
                .map(|log_entry| log.push_log_entry(bookmark, log_entry));
            let (txn_, result) =
                DeleteBookmarkIf::query_with_transaction(txn, &self.repo_id, bookmark, old_cs_id)
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
        txn: SqlTransaction,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        let (mut txn, next_id) = Self::find_next_update_log_id(txn).await?;

        let mut log = TransactionLogUpdates::new(next_id);

        txn = self.store_force_sets(txn, &mut log).await?;
        txn = self.store_creates(txn, &mut log).await?;
        txn = self.store_updates(txn, &mut log).await?;
        txn = self.store_force_deletes(txn, &mut log).await?;
        txn = self.store_deletes(txn, &mut log).await?;
        txn = self
            .store_log(txn, &log)
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
        bookmark: &BookmarkName,
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
        bookmark: &BookmarkName,
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

    fn create_publishing(
        &mut self,
        bookmark: &BookmarkName,
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

    fn create_scratch(&mut self, bookmark: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
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
        bookmark: &BookmarkName,
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
        bookmark: &BookmarkName,
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

    fn force_delete(
        &mut self,
        bookmark: &BookmarkName,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        let log = NewUpdateLogEntry::new(None, None, reason)?;
        self.payload.force_deletes.push((bookmark.clone(), log));
        Ok(())
    }

    fn delete_scratch(&mut self, bookmark: &BookmarkName, old_cs: ChangesetId) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.deletes.push((
            bookmark.clone(),
            old_cs,
            None, // Scratch bookmark updates are not logged.
        ));
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

                let mut txn = write_connection.start_transaction().await?;

                txn = match txn_hook(ctx.clone(), txn).await {
                    Ok(txn) => txn,
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue;
                    }
                    err => break err,
                };

                match payload.attempt_write(txn).await {
                    Err(BookmarkTransactionError::RetryableError(_))
                        if attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT =>
                    {
                        continue;
                    }
                    result => break result,
                }
            };

            match result {
                Ok(txn) => {
                    STATS::bookmarks_update_log_insert_success.add_value(1);
                    STATS::bookmarks_update_log_insert_success_attempt_count
                        .add_value(attempt as i64);
                    txn.commit().await?;
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
    rows: impl IntoIterator<Item = (&RepositoryId, &BookmarkName, &ChangesetId, &BookmarkKind)>,
) -> Result<()> {
    let none = None;
    let rows = rows
        .into_iter()
        .map(|(r, b, c, k)| (r, &none, b, c, k))
        .collect::<Vec<_>>();
    InsertBookmarks::query(conn, rows.as_slice()).await?;
    Ok(())
}
