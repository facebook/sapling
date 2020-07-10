/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, bail, Error, Result};
use bookmarks::{
    Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, BookmarkTransaction,
    BookmarkTransactionError, BookmarkTransactionHook, BookmarkUpdateLog, BookmarkUpdateLogEntry,
    BookmarkUpdateReason, Bookmarks, BundleReplayData, Freshness,
};
use context::{CoreContext, PerfCounterType};
use futures::compat::Future01CompatExt;
use futures::future::{self, BoxFuture, Future, FutureExt, TryFutureExt};
use futures::stream::{self, BoxStream, StreamExt, TryStreamExt};
use mononoke_types::Timestamp;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection, Transaction as SqlTransaction};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT: usize = 5;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list: timeseries(Rate, Sum),
    list_maybe_stale: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
    bookmarks_update_log_insert_success: timeseries(Rate, Sum),
    bookmarks_update_log_insert_success_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_retryable_error: timeseries(Rate, Sum),
    bookmarks_insert_retryable_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_logic_error: timeseries(Rate, Sum),
    bookmarks_insert_logic_error_attempt_count: timeseries(Rate, Average, Sum),
    bookmarks_insert_other_error: timeseries(Rate, Sum),
    bookmarks_insert_other_error_attempt_count: timeseries(Rate, Average, Sum),
}

#[derive(Clone)]
pub struct SqlBookmarks {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
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

    read ReadNextBookmarkLogEntries(min_id: u64, repo_id: RepositoryId, limit: u64) -> (
        i64, RepositoryId, BookmarkName, Option<ChangesetId>, Option<ChangesetId>,
        BookmarkUpdateReason, Timestamp, Option<String>, Option<String>
    ) {
        "SELECT id, repo_id, name, to_changeset_id, from_changeset_id, reason, timestamp,
              replay.bundle_handle, replay.commit_hashes_json
         FROM bookmarks_update_log log
         LEFT JOIN bundle_replay_data replay ON log.id = replay.bookmark_update_log_id
         WHERE log.id > {min_id} AND log.repo_id = {repo_id}
         ORDER BY id asc
         LIMIT {limit}"
    }

    read CountFurtherBookmarkLogEntries(min_id: u64, repo_id: RepositoryId) -> (u64) {
        "SELECT COUNT(*)
        FROM bookmarks_update_log
        WHERE id > {min_id} AND repo_id = {repo_id}"
    }

    read CountFurtherBookmarkLogEntriesByReason(min_id: u64, repo_id: RepositoryId) -> (BookmarkUpdateReason, u64) {
        "SELECT reason, COUNT(*)
        FROM bookmarks_update_log
        WHERE id > {min_id} AND repo_id = {repo_id}
        GROUP BY reason"
    }

    read SkipOverBookmarkLogEntriesWithReason(min_id: u64, repo_id: RepositoryId, reason: BookmarkUpdateReason) -> (u64) {
        // We find the first entry that we _don't_ want to skip.
        // Then we find the first entry that we do want to skip and is immediately before this.
        // We don't allow looking back, so if we're going backwards, nothing happens.
        "
        SELECT id
        FROM bookmarks_update_log
        WHERE
            repo_id = {repo_id} AND
            id > {min_id} AND
            reason = {reason} AND
            id < (
                SELECT id
                FROM bookmarks_update_log
                WHERE
                    repo_id = {repo_id} AND
                    id > {min_id} AND
                    NOT reason = {reason}
                ORDER BY id ASC
                LIMIT 1
            )
        ORDER BY id DESC
        LIMIT 1
        "
    }

    read FindMaxBookmarkLogId() -> (Option<u64>) {
        "SELECT MAX(id) FROM bookmarks_update_log"
    }

    read CountFurtherBookmarkLogEntriesWithoutReason(min_id: u64, repo_id: RepositoryId, reason: BookmarkUpdateReason) -> (u64) {
        "SELECT COUNT(*)
        FROM bookmarks_update_log
        WHERE id > {min_id} AND repo_id = {repo_id} AND NOT reason = {reason}"
    }

    write AddBundleReplayData(values: (id: u64, bundle_handle: String, commit_hashes_json: String)) {
        none,
        "INSERT INTO bundle_replay_data
         (bookmark_update_log_id, bundle_handle, commit_hashes_json)
         VALUES {values}"
    }

    read SelectBookmark(repo_id: RepositoryId, name: BookmarkName) -> (ChangesetId) {
        "SELECT changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
         LIMIT 1"
    }

    read SelectBookmarkLogs(repo_id: RepositoryId, name: BookmarkName, max_records: u32) -> (
        Option<ChangesetId>, BookmarkUpdateReason, Timestamp
    ) {
        "SELECT to_changeset_id, reason, timestamp
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}
           AND name = {name}
         ORDER BY id DESC
         LIMIT {max_records}"
      }

    read SelectBookmarkLogsWithOffset(repo_id: RepositoryId, name: BookmarkName, max_records: u32, offset: u32) -> (
        Option<ChangesetId>, BookmarkUpdateReason, Timestamp
    ) {
        "SELECT to_changeset_id, reason, timestamp
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}
           AND name = {name}
         ORDER BY id DESC
         LIMIT {max_records}
         OFFSET {offset}"
      }

    read SelectAll(
        repo_id: RepositoryId,
        limit: u64,
        >list kinds: BookmarkKind
    ) -> (BookmarkName, BookmarkKind, ChangesetId) {
        "SELECT name, hg_kind, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    read SelectAllAfter(
        repo_id: RepositoryId,
        after: BookmarkName,
        limit: u64,
        >list kinds: BookmarkKind
    ) -> (BookmarkName, BookmarkKind, ChangesetId) {
        "SELECT name, hg_kind, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name > {after}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    read SelectByPrefix(
        repo_id: RepositoryId,
        prefix_like_pattern: String,
        escape_character: &str,
        limit: u64,
        >list kinds: BookmarkKind
    ) -> (BookmarkName, BookmarkKind, ChangesetId) {
        "SELECT name, hg_kind, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name LIKE {prefix_like_pattern} ESCAPE {escape_character}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    read SelectByPrefixAfter(
        repo_id: RepositoryId,
        prefix_like_pattern: String,
        escape_character: &str,
        after: BookmarkName,
        limit: u64,
        >list kinds: BookmarkKind
    ) -> (BookmarkName, BookmarkKind, ChangesetId) {
        "SELECT name, hg_kind, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name LIKE {prefix_like_pattern} ESCAPE {escape_character}
           AND name > {after}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }
}

impl SqlConstruct for SqlBookmarks {
    const LABEL: &'static str = "bookmarks";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bookmarks.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBookmarks {}

fn query_to_stream<F>(query: F) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>>
where
    F: Future<Output = Result<Vec<(BookmarkName, BookmarkKind, ChangesetId)>>> + Send + 'static,
{
    query
        .map_ok(move |rows| stream::iter(rows.into_iter().map(Ok)))
        .try_flatten_stream()
        .map_ok(|row| {
            let (name, kind, changeset_id) = row;
            (Bookmark::new(name, kind), changeset_id)
        })
        .boxed()
}

impl Bookmarks for SqlBookmarks {
    fn list(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        freshness: Freshness,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        let conn = match freshness {
            Freshness::MaybeStale => {
                STATS::list_maybe_stale.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
                &self.read_connection
            }
            Freshness::MostRecent => {
                STATS::list.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                &self.read_master_connection
            }
        };

        if prefix.is_empty() {
            match pagination {
                BookmarkPagination::FromStart => {
                    query_to_stream(SelectAll::query(&conn, &repo_id, &limit, kinds).compat())
                }
                BookmarkPagination::After(ref after) => query_to_stream(
                    SelectAllAfter::query(&conn, &repo_id, after, &limit, kinds).compat(),
                ),
            }
        } else {
            let prefix_like_pattern = prefix.to_escaped_sql_like_pattern();
            match pagination {
                BookmarkPagination::FromStart => query_to_stream(
                    SelectByPrefix::query(
                        &conn,
                        &repo_id,
                        &prefix_like_pattern,
                        &"\\",
                        &limit,
                        kinds,
                    )
                    .compat(),
                ),
                BookmarkPagination::After(ref after) => query_to_stream(
                    SelectByPrefixAfter::query(
                        &conn,
                        &repo_id,
                        &prefix_like_pattern,
                        &"\\",
                        after,
                        &limit,
                        kinds,
                    )
                    .compat(),
                ),
            }
        }
    }

    fn get(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
        repo_id: RepositoryId,
    ) -> BoxFuture<'static, Result<Option<ChangesetId>>> {
        STATS::get_bookmark.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        SelectBookmark::query(&self.read_master_connection, &repo_id, &name)
            .compat()
            .map_ok(|rows| rows.into_iter().next().map(|row| row.0))
            .boxed()
    }

    fn create_transaction(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Box<dyn BookmarkTransaction> {
        Box::new(SqlBookmarksTransaction::new(
            ctx,
            self.write_connection.clone(),
            repo_id.clone(),
        ))
    }
}

impl BookmarkUpdateLog for SqlBookmarks {
    fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        repo_id: RepositoryId,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            &self.read_master_connection
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            &self.read_connection
        };

        match offset {
            Some(offset) => {
                SelectBookmarkLogsWithOffset::query(&connection, &repo_id, &name, &max_rec, &offset)
                    .compat()
                    .map_ok(|rows| stream::iter(rows.into_iter().map(Ok)))
                    .try_flatten_stream()
                    .boxed()
            }
            None => SelectBookmarkLogs::query(&connection, &repo_id, &name, &max_rec)
                .compat()
                .map_ok(|rows| stream::iter(rows.into_iter().map(Ok)))
                .try_flatten_stream()
                .boxed(),
        }
    }

    fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        maybe_exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<'static, Result<u64>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let query = match maybe_exclude_reason {
            Some(ref r) => CountFurtherBookmarkLogEntriesWithoutReason::query(
                &self.read_connection,
                &id,
                &repoid,
                &r,
            )
            .compat()
            .boxed(),
            None => CountFurtherBookmarkLogEntries::query(&self.read_connection, &id, &repoid)
                .compat()
                .boxed(),
        };

        query
            .and_then(move |entries| {
                let entry = entries.into_iter().next();
                match entry {
                    Some(count) => future::ok(count.0),
                    None => {
                        let extra = match maybe_exclude_reason {
                            Some(..) => "without reason",
                            None => "",
                        };
                        let msg = format!("Failed to query further bookmark log entries{}", extra);
                        future::err(Error::msg(msg))
                    }
                }
            })
            .boxed()
    }

    fn count_further_bookmark_log_entries_by_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<'static, Result<Vec<(BookmarkUpdateReason, u64)>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        CountFurtherBookmarkLogEntriesByReason::query(&self.read_connection, &id, &repoid)
            .compat()
            .map_ok(|entries| entries.into_iter().collect())
            .boxed()
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<'static, Result<Option<u64>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        SkipOverBookmarkLogEntriesWithReason::query(&self.read_connection, &id, &repoid, &reason)
            .compat()
            .map_ok(|entries| entries.first().map(|entry| entry.0))
            .boxed()
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        ReadNextBookmarkLogEntries::query(&self.read_connection, &id, &repoid, &limit)
            .compat()
            .map_ok(|entries| {
                let homogenous_entries: Vec<_> = match entries.iter().nth(0).cloned() {
                    Some(first_entry) => {
                        // Note: types are explicit here to protect us from query behavior change
                        //       when tuple items 2 or 5 become something else, and we still succeed
                        //       compiling everything because of the type inference
                        let first_name: &BookmarkName = &first_entry.2;
                        let first_reason: &BookmarkUpdateReason = &first_entry.5;
                        entries
                            .into_iter()
                            .take_while(|entry| {
                                let name: &BookmarkName = &entry.2;
                                let reason: &BookmarkUpdateReason = &entry.5;
                                name == first_name && reason == first_reason
                            })
                            .collect()
                    }
                    None => entries.into_iter().collect(),
                };
                stream::iter(homogenous_entries.into_iter().map(Ok)).and_then(|entry| async move {
                    let (
                        id,
                        repo_id,
                        name,
                        to_cs_id,
                        from_cs_id,
                        reason,
                        timestamp,
                        bundle_handle,
                        commit_timestamps,
                    ) = entry;
                    get_bundle_replay_data(bundle_handle, commit_timestamps).and_then(
                        |replay_data| {
                            Ok(BookmarkUpdateLogEntry {
                                id,
                                repo_id,
                                bookmark_name: name,
                                to_changeset_id: to_cs_id,
                                from_changeset_id: from_cs_id,
                                reason: reason.update_bundle_replay_data(replay_data)?,
                                timestamp,
                            })
                        },
                    )
                })
            })
            .try_flatten_stream()
            .boxed()
    }

    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            &self.read_master_connection
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            &self.read_connection
        };

        ReadNextBookmarkLogEntries::query(&connection, &id, &repoid, &limit)
            .compat()
            .map_ok(|entries| {
                stream::iter(entries.into_iter().map(Ok)).and_then(|entry| async move {
                    let (
                        id,
                        repo_id,
                        name,
                        to_cs_id,
                        from_cs_id,
                        reason,
                        timestamp,
                        bundle_handle,
                        commit_timestamps,
                    ) = entry;
                    get_bundle_replay_data(bundle_handle, commit_timestamps).and_then(
                        |replay_data| {
                            Ok(BookmarkUpdateLogEntry {
                                id,
                                repo_id,
                                bookmark_name: name,
                                to_changeset_id: to_cs_id,
                                from_changeset_id: from_cs_id,
                                reason: reason.update_bundle_replay_data(replay_data)?,
                                timestamp,
                            })
                        },
                    )
                })
            })
            .try_flatten_stream()
            .boxed()
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

    async fn log_bundle_replay_data(
        id: u64,
        reason: BookmarkUpdateReason,
        sql_transaction: SqlTransaction,
    ) -> Result<SqlTransaction> {
        use BookmarkUpdateReason::*;
        let sql_transaction = match reason {
            Pushrebase {
                bundle_replay_data: Some(bundle_replay_data),
            }
            | Push {
                bundle_replay_data: Some(bundle_replay_data),
            }
            | TestMove {
                bundle_replay_data: Some(bundle_replay_data),
            }
            | Backsyncer {
                bundle_replay_data: Some(bundle_replay_data),
            } => {
                let BundleReplayData {
                    bundle_handle,
                    commit_timestamps,
                } = bundle_replay_data;
                let commit_timestamps = serde_json::to_string(&commit_timestamps)?;

                AddBundleReplayData::query_with_transaction(
                    sql_transaction,
                    &[(&id, &bundle_handle, &commit_timestamps)],
                )
                .compat()
                .await?
                .0
            }
            Pushrebase {
                bundle_replay_data: None,
            }
            | Push {
                bundle_replay_data: None,
            }
            | TestMove {
                bundle_replay_data: None,
            }
            | Backsyncer {
                bundle_replay_data: None,
            }
            | ManualMove
            | Blobimport
            | XRepoSync => sql_transaction,
        };
        Ok(sql_transaction)
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
            txn = Self::log_bundle_replay_data(next_id, log_entry.reason.clone(), txn).await?;
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
    fn new(ctx: CoreContext, write_connection: Connection, repo_id: RepositoryId) -> Self {
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
        self.payload.updates.insert(
            bookmark.clone(),
            (old_cs, new_cs, BookmarkKind::ALL_PUBLISHING),
        );
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(Some(old_cs), Some(new_cs), reason)?,
        );
        Ok(())
    }

    fn create(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.creates.insert(
            bookmark.clone(),
            (new_cs, BookmarkKind::PullDefaultPublishing),
        );
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, Some(new_cs), reason)?,
        );
        Ok(())
    }

    fn force_set(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.force_sets.insert(bookmark.clone(), new_cs);
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, Some(new_cs), reason)?,
        );
        Ok(())
    }

    fn delete(
        &mut self,
        bookmark: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.deletes.insert(bookmark.clone(), old_cs);
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(Some(old_cs), None, reason)?,
        );
        Ok(())
    }

    fn force_delete(
        &mut self,
        bookmark: &BookmarkName,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_not_seen(bookmark)?;
        self.payload.force_deletes.insert(bookmark.clone());
        self.payload.log.insert(
            bookmark.clone(),
            NewUpdateLogEntry::new(None, None, reason)?,
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

fn get_bundle_replay_data(
    bundle_handle: Option<String>,
    commit_timestamps: Option<String>,
) -> Result<Option<BundleReplayData>> {
    match (bundle_handle, commit_timestamps) {
        (Some(bundle_handle), Some(commit_timestamps)) => {
            let replay_data = BundleReplayData {
                bundle_handle,
                commit_timestamps: serde_json::from_str(&commit_timestamps)?,
            };
            Ok(Some(replay_data))
        }
        (None, None) => Ok(None),
        _ => bail!("inconsistent replay data"),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ascii::AsciiString;
    use fbinit::FacebookInit;
    use mononoke_types_mocks::{
        changesetid::{ONES_CSID, TWOS_CSID},
        repo::REPO_ZERO,
    };
    use quickcheck::quickcheck;
    use std::collections::{BTreeMap, HashSet};
    use tokio_compat::runtime::Runtime;

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book.to_string()).unwrap()
    }

    #[fbinit::compat_test]
    async fn test_update_kind_compatibility(fb: FacebookInit) -> Result<()> {
        let data = BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        };

        let ctx = CoreContext::test_mock(fb);
        let store = SqlBookmarks::with_sqlite_in_memory().unwrap();
        let scratch_name = create_bookmark_name("book1");
        let publishing_name = create_bookmark_name("book2");
        let pull_default_name = create_bookmark_name("book3");

        let conn = store.write_connection.clone();

        let rows = vec![
            (
                &REPO_ZERO,
                &scratch_name,
                &ONES_CSID,
                &BookmarkKind::Scratch,
            ),
            (
                &REPO_ZERO,
                &publishing_name,
                &ONES_CSID,
                &BookmarkKind::Publishing,
            ),
            (
                &REPO_ZERO,
                &pull_default_name,
                &ONES_CSID,
                &BookmarkKind::PullDefaultPublishing,
            ),
        ];

        InsertBookmarks::query(&conn, &rows[..]).compat().await?;

        // Using 'create_scratch' to replace a non-scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create_scratch(&publishing_name, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'create' to replace a scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create(&scratch_name, ONES_CSID, data.clone())?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a publishing bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_scratch(&publishing_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a pull-default bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_scratch(&pull_default_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'update' to update a publishing bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&publishing_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(txn.commit().await?);

        // Using 'update' to update a pull-default bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&pull_default_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(txn.commit().await?);

        // Using 'update' to update a scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&scratch_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a scratch bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_scratch(&scratch_name, TWOS_CSID, ONES_CSID)?;
        assert!(txn.commit().await?);

        Ok(())
    }

    fn mock_bookmarks_response(
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let range = prefix.to_range().with_pagination(pagination.clone());
        bookmarks
            .range(range)
            .filter_map(|(bookmark, (kind, changeset_id))| {
                if kinds.iter().any(|k| kind == k) {
                    let bookmark = Bookmark {
                        name: bookmark.clone(),
                        kind: *kind,
                    };
                    Some((bookmark, *changeset_id))
                } else {
                    None
                }
            })
            .take(limit as usize)
            .collect()
    }

    fn insert_then_query(
        fb: FacebookInit,
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        query_freshness: Freshness,
        query_prefix: &BookmarkPrefix,
        query_kinds: &[BookmarkKind],
        query_pagination: &BookmarkPagination,
        query_limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let mut rt = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(123);

        let store = SqlBookmarks::with_sqlite_in_memory().unwrap();
        let conn = store.write_connection.clone();

        let rows: Vec<_> = bookmarks
            .iter()
            .map(|(bookmark, (kind, changeset_id))| (&repo_id, bookmark, changeset_id, kind))
            .collect();

        rt.block_on(InsertBookmarks::query(&conn, rows.as_slice()))
            .expect("insert failed");

        let response = store
            .list(
                ctx,
                repo_id,
                query_freshness,
                query_prefix,
                query_kinds,
                query_pagination,
                query_limit,
            )
            .try_collect::<Vec<_>>();

        rt.block_on_std(response).expect("query failed")
    }

    quickcheck! {
        fn responses_match(
            fb: FacebookInit,
            bookmarks: BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
            freshness: Freshness,
            kinds: HashSet<BookmarkKind>,
            prefix_char: Option<ascii_ext::AsciiChar>,
            after: Option<BookmarkName>,
            limit: u64
        ) -> bool {
            // Test that requests return what is expected.
            let kinds: Vec<_> = kinds.into_iter().collect();
            let prefix = match prefix_char {
                Some(ch) => BookmarkPrefix::new_ascii(AsciiString::from(&[ch.0][..])),
                None => BookmarkPrefix::empty(),
            };
            let pagination = match after {
                Some(name) => BookmarkPagination::After(name),
                None => BookmarkPagination::FromStart,
            };
            let have = insert_then_query(
                fb,
                &bookmarks,
                freshness,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            let want = mock_bookmarks_response(
                &bookmarks,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            have == want
        }
    }
}
