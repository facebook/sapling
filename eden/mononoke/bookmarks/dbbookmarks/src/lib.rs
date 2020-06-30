/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{bail, format_err, Error, Result};
use bookmarks::{
    Bookmark, BookmarkHgKind, BookmarkName, BookmarkPrefix, BookmarkTransactionError,
    BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks, BundleReplayData, Freshness,
    Transaction, TransactionHook,
};
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use futures::compat::Future01CompatExt;
use futures::future::{self, BoxFuture, Future, FutureExt, TryFutureExt};
use futures::stream::{self, BoxStream, StreamExt, TryStreamExt};
use mononoke_types::Timestamp;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection, Transaction as SqlTransaction};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::{SqlConnections, TransactionResult};
use stats::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

const DEFAULT_MAX: u64 = std::u64::MAX;
const MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT: usize = 5;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list_all_by_prefix: timeseries(Rate, Sum),
    list_all_by_prefix_maybe_stale: timeseries(Rate, Sum),
    list_pull_default_by_prefix: timeseries(Rate, Sum),
    list_pull_default_by_prefix_maybe_stale: timeseries(Rate, Sum),
    list_publishing_by_prefix: timeseries(Rate, Sum),
    list_publishing_by_prefix_maybe_stale: timeseries(Rate, Sum),
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
        values: (repo_id: RepositoryId, name: BookmarkName, changeset_id: ChangesetId, hg_kind: BookmarkHgKind)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmarks (repo_id, name, changeset_id, hg_kind) VALUES {values}"
    }

    write UpdateBookmark(
        repo_id: RepositoryId,
        name: BookmarkName,
        old_id: ChangesetId,
        new_id: ChangesetId,
        >list kinds: BookmarkHgKind
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

    read SelectAll(repo_id: RepositoryId, limit: u64, >list hg_kind: BookmarkHgKind) ->  (BookmarkName, BookmarkHgKind, ChangesetId) {
        "SELECT name, hg_kind, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND hg_kind IN {hg_kind}
         LIMIT {limit}"
    }

    read SelectByPrefix(repo_id: RepositoryId, prefix: BookmarkPrefix, limit: u64, >list hg_kind: BookmarkHgKind) ->  (BookmarkName, BookmarkHgKind, ChangesetId) {
        mysql(
            "SELECT name, hg_kind, changeset_id
             FROM bookmarks
             WHERE repo_id = {repo_id}
               AND name LIKE CONCAT({prefix}, '%')
               AND hg_kind IN {hg_kind}
              LIMIT {limit}"
        )
        sqlite(
            "SELECT name, hg_kind, changeset_id
             FROM bookmarks
             WHERE repo_id = {repo_id}
               AND name LIKE {prefix} || '%'
               AND hg_kind IN {hg_kind}
             LIMIT {limit}"
        )
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

fn query_to_stream<F>(v: F, max: u64) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>>
where
    F: Future<Output = Result<Vec<(BookmarkName, BookmarkHgKind, ChangesetId)>>> + Send + 'static,
{
    v.map_ok(move |rows| {
        if rows.len() as u64 >= max {
            let message = format_err!(
                "Bookmark query was truncated after {} results, use a more specific prefix search.",
                max
            );
            future::err(message).into_stream().left_stream()
        } else {
            stream::iter(rows.into_iter().map(Ok)).right_stream()
        }
    })
    .try_flatten_stream()
    .map_ok(|row| {
        let (name, hg_kind, changeset_id) = row;
        (Bookmark::new(name, hg_kind), changeset_id)
    })
    .boxed()
}

impl SqlBookmarks {
    fn list_impl(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkHgKind],
        freshness: Freshness,
        max: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        let conn = match freshness {
            Freshness::MostRecent => &self.read_master_connection,
            Freshness::MaybeStale => &self.read_connection,
        };

        let query = if prefix.is_empty() {
            SelectAll::query(&conn, &repo_id, &max, kinds)
                .compat()
                .left_future()
        } else {
            SelectByPrefix::query(&conn, &repo_id, &prefix, &max, kinds)
                .compat()
                .right_future()
        };

        query_to_stream(query, max)
    }
}

impl Bookmarks for SqlBookmarks {
    fn list_publishing_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        match freshness {
            Freshness::MaybeStale => {
                STATS::list_publishing_by_prefix_maybe_stale.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
            }
            Freshness::MostRecent => {
                STATS::list_publishing_by_prefix.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
            }
        };

        use BookmarkHgKind::*;
        let kinds = vec![PublishingNotPullDefault, PullDefault];
        self.list_impl(ctx, repo_id, prefix, &kinds, freshness, DEFAULT_MAX)
    }

    fn list_pull_default_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        match freshness {
            Freshness::MaybeStale => {
                STATS::list_pull_default_by_prefix_maybe_stale.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
            }
            Freshness::MostRecent => {
                STATS::list_pull_default_by_prefix.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
            }
        };

        use BookmarkHgKind::*;
        let kinds = vec![PullDefault];
        self.list_impl(ctx, repo_id, prefix, &kinds, freshness, DEFAULT_MAX)
    }

    fn list_all_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
        max: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        match freshness {
            Freshness::MaybeStale => {
                STATS::list_all_by_prefix_maybe_stale.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
            }
            Freshness::MostRecent => {
                STATS::list_all_by_prefix.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
            }
        };

        use BookmarkHgKind::*;
        let kinds = vec![Scratch, PublishingNotPullDefault, PullDefault];
        self.list_impl(ctx, repo_id, prefix, &kinds, freshness, max)
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

    fn create_transaction(&self, ctx: CoreContext, repoid: RepositoryId) -> Box<dyn Transaction> {
        Box::new(SqlBookmarksTransaction::new(
            ctx,
            self.write_connection.clone(),
            repoid.clone(),
        ))
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

impl SqlBookmarks {
    pub async fn write_to_txn(
        &self,
        ctx: CoreContext,
        repoid: RepositoryId,
        sql_txn: SqlTransaction,
        bookmark: BookmarkName,
        from_changeset_id: Option<ChangesetId>,
        to_changeset_id: Option<ChangesetId>,
        reason: BookmarkUpdateReason,
    ) -> Result<TransactionResult> {
        let mut book_txn =
            SqlBookmarksTransaction::new(ctx, self.write_connection.clone(), repoid.clone());

        match (from_changeset_id, to_changeset_id) {
            (Some(from_cs_id), Some(to_cs_id)) => {
                book_txn.update(&bookmark, to_cs_id, from_cs_id, reason)?;
            }
            (Some(from_cs_id), None) => {
                book_txn.delete(&bookmark, from_cs_id, reason)?;
            }
            (None, Some(to_cs_id)) => {
                // Unfortunately we can't tell if a bookmark was created or force set.
                // Because of that we have to always do force set.
                book_txn.force_set(&bookmark, to_cs_id, reason)?;
            }
            (None, None) => {
                return Err(Error::msg("unsupported bookmark move"));
            }
        };

        book_txn.update_transaction(sql_txn).await
    }
}

type RetryAttempt = usize;

async fn conditional_retry_without_delay<V, Fut, RetryableFunc, DecisionFunc>(
    func: RetryableFunc,
    should_retry: DecisionFunc,
) -> Result<(V, RetryAttempt), (BookmarkTransactionError, RetryAttempt)>
where
    V: Send + 'static,
    Fut: Future<Output = Result<V, BookmarkTransactionError>>,
    RetryableFunc: Fn(RetryAttempt) -> Fut + Send + 'static,
    DecisionFunc: Fn(&BookmarkTransactionError, RetryAttempt) -> bool + Send + 'static,
{
    for attempt in 1.. {
        match func(attempt).await {
            Ok(res) => return Ok((res, attempt)),
            Err(err) => {
                if !should_retry(&err, attempt) {
                    return Err((err, attempt));
                }
            }
        }
    }
    unreachable!()
}

#[derive(Clone, Default)]
struct SqlBookmarksTransactionPayload {
    force_sets: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    creates: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    sets: HashMap<BookmarkName, (BookmarkSetData, BookmarkUpdateReason)>,
    force_deletes: HashMap<BookmarkName, BookmarkUpdateReason>,
    deletes: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    infinitepush_sets: HashMap<BookmarkName, BookmarkSetData>,
    infinitepush_creates: HashMap<BookmarkName, ChangesetId>,
}

impl SqlBookmarksTransactionPayload {
    pub fn log_rows(
        &self,
    ) -> HashMap<
        BookmarkName,
        (
            Option<ChangesetId>,
            Option<ChangesetId>,
            BookmarkUpdateReason,
        ),
    > {
        let mut log_rows = HashMap::new();

        for (bookmark, (to_cs_id, reason)) in &self.force_sets {
            log_rows.insert(bookmark.clone(), (None, Some(*to_cs_id), reason.clone()));
        }

        for (bookmark, (to_cs_id, reason)) in &self.creates {
            log_rows.insert(bookmark.clone(), (None, Some(*to_cs_id), reason.clone()));
        }

        for (bookmark, (bookmark_set_data, reason)) in &self.sets {
            let from_cs_id = bookmark_set_data.old_cs;
            let to_cs_id = bookmark_set_data.new_cs;
            log_rows.insert(
                bookmark.clone(),
                (Some(from_cs_id), Some(to_cs_id), reason.clone()),
            );
        }

        for (bookmark, reason) in &self.force_deletes {
            log_rows.insert(bookmark.clone(), (None, None, reason.clone()));
        }

        for (bookmark, (from_cs_id, reason)) in &self.deletes {
            log_rows.insert(bookmark.clone(), (Some(*from_cs_id), None, reason.clone()));
        }

        log_rows
    }

    pub fn check_if_bookmark_already_used(&self, key: &BookmarkName) -> Result<()> {
        if self.creates.contains_key(key)
            || self.force_sets.contains_key(key)
            || self.sets.contains_key(key)
            || self.force_deletes.contains_key(key)
            || self.deletes.contains_key(key)
            || self.infinitepush_sets.contains_key(key)
            || self.infinitepush_creates.contains_key(key)
        {
            bail!("{} bookmark was already used", key);
        }
        Ok(())
    }
}

pub struct SqlBookmarksTransaction {
    write_connection: Connection,
    repo_id: RepositoryId,
    ctx: CoreContext,
    payload: SqlBookmarksTransactionPayload,
}

impl SqlBookmarksTransaction {
    fn new(ctx: CoreContext, write_connection: Connection, repo_id: RepositoryId) -> Self {
        Self {
            write_connection,
            repo_id,
            ctx,
            payload: SqlBookmarksTransactionPayload::default(),
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

    async fn find_next_update_log_id(
        sql_transaction: SqlTransaction,
    ) -> Result<(SqlTransaction, u64)> {
        let (sql_transaction, max_id_entries) =
            FindMaxBookmarkLogId::query_with_transaction(sql_transaction)
                .compat()
                .await?;

        let next_id = 1 + match &max_id_entries[..] {
            [(None,)] => 0,
            [(Some(max_existing),)] => *max_existing,
            _ => {
                return Err(format_err!(
                    "Should be impossible. FindMaxBookmarkLogId returned not a single entry: {:?}",
                    max_id_entries
                ))
            }
        };
        Ok((sql_transaction, next_id))
    }

    async fn log_bookmark_moves(
        repo_id: RepositoryId,
        timestamp: Timestamp,
        moves: HashMap<
            BookmarkName,
            (
                Option<ChangesetId>,
                Option<ChangesetId>,
                BookmarkUpdateReason,
            ),
        >,
        sql_transaction: SqlTransaction,
    ) -> Result<SqlTransaction> {
        let (mut sql_transaction, mut next_id) =
            Self::find_next_update_log_id(sql_transaction).await?;
        for (bookmark, (from_changeset_id, to_changeset_id, reason)) in moves {
            let row = vec![(
                &next_id,
                &repo_id,
                &bookmark,
                &from_changeset_id,
                &to_changeset_id,
                &reason,
                &timestamp,
            )];
            let reason = reason.clone();
            sql_transaction =
                AddBookmarkLog::query_with_transaction(sql_transaction, row.as_slice())
                    .compat()
                    .await?
                    .0;
            sql_transaction =
                Self::log_bundle_replay_data(next_id, reason, sql_transaction).await?;
            next_id += 1;
        }
        Ok(sql_transaction)
    }

    async fn attempt_write(
        transaction: SqlTransaction,
        repo_id: RepositoryId,
        payload: SqlBookmarksTransactionPayload,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        // NOTE: Infinitepush updates do *not* go into log_rows. This is because the
        // BookmarkUpdateLog is currently used for replays to Mercurial, and those updates should
        // not be replayed (for Infinitepush, those updates are actually dispatched by the client
        // to both destination).
        let log_rows = payload.log_rows();

        let SqlBookmarksTransactionPayload {
            force_sets,
            creates,
            sets,
            force_deletes,
            deletes,
            infinitepush_sets,
            infinitepush_creates,
        } = payload;

        let force_set: Vec<_> = force_sets.clone().into_iter().collect();
        let mut ref_rows = Vec::new();
        for idx in 0..force_set.len() {
            let (ref to_changeset_id, _) = force_set[idx].1;
            ref_rows.push((&repo_id, &force_set[idx].0, to_changeset_id));
        }

        let (txn, _) = ReplaceBookmarks::query_with_transaction(transaction, &ref_rows[..])
            .compat()
            .await?;

        let mut ref_rows = Vec::new();

        let creates_vec: Vec<_> = creates.clone().into_iter().collect();
        for idx in 0..creates_vec.len() {
            let (ref to_changeset_id, _) = creates_vec[idx].1;
            ref_rows.push((
                &repo_id,
                &creates_vec[idx].0,
                to_changeset_id,
                &BookmarkHgKind::PullDefault,
            ))
        }

        for (name, cs_id) in infinitepush_creates.iter() {
            ref_rows.push((&repo_id, &name, cs_id, &BookmarkHgKind::Scratch));
        }

        let rows_to_insert = ref_rows.len() as u64;
        let (mut txn, result) = InsertBookmarks::query_with_transaction(txn, &ref_rows[..])
            .compat()
            .await?;

        if result.affected_rows() != rows_to_insert {
            return Err(BookmarkTransactionError::LogicError);
        }

        // Iterate over (BookmarkName, BookmarkSetData, *Allowed Kinds to update from)
        // We allow up to 2 kinds to update from.
        use BookmarkHgKind::*;

        let sets_iter = sets.into_iter().map(|(name, (data, _reason))| {
            (name, data, vec![PullDefault, PublishingNotPullDefault])
        });

        let infinitepush_sets_iter = infinitepush_sets
            .into_iter()
            .map(|(name, data)| (name, data, vec![Scratch]));

        let updates_iter = sets_iter.chain(infinitepush_sets_iter);

        for (
            ref name,
            BookmarkSetData {
                ref new_cs,
                ref old_cs,
            },
            ref kinds,
        ) in updates_iter
        {
            if new_cs == old_cs {
                // no-op update. If bookmark points to a correct update then
                // let's continue the transaction, otherwise revert it
                let (txn_, result) = SelectBookmark::query_with_transaction(txn, &repo_id, &name)
                    .compat()
                    .await?;
                txn = txn_;
                let new_cs = new_cs.clone();
                if result.get(0).map(|b| b.0) != Some(new_cs) {
                    return Err(BookmarkTransactionError::LogicError);
                }
            } else {
                let (txn_, result) = UpdateBookmark::query_with_transaction(
                    txn,
                    &repo_id,
                    &name,
                    &old_cs,
                    &new_cs,
                    &kinds[..],
                )
                .compat()
                .await?;
                txn = txn_;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
            }
        }

        for (name, _reason) in force_deletes {
            let (txn_, _) = DeleteBookmark::query_with_transaction(txn, &repo_id, &name)
                .compat()
                .await?;
            txn = txn_;
        }
        for (name, (old_cs, _reason)) in deletes {
            let (txn_, result) =
                DeleteBookmarkIf::query_with_transaction(txn, &repo_id, &name, &old_cs)
                    .compat()
                    .await?;
            txn = txn_;

            if result.affected_rows() != 1 {
                return Err(BookmarkTransactionError::LogicError);
            }
        }

        Self::log_bookmark_moves(repo_id, Timestamp::now(), log_rows, txn)
            .await
            .map_err(BookmarkTransactionError::RetryableError)
    }
}

impl Transaction for SqlBookmarksTransaction {
    fn update(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload
            .sets
            .insert(key.clone(), (BookmarkSetData { new_cs, old_cs }, reason));
        Ok(())
    }

    fn create(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload.creates.insert(key.clone(), (new_cs, reason));
        Ok(())
    }

    fn force_set(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload
            .force_sets
            .insert(key.clone(), (new_cs, reason));
        Ok(())
    }

    fn delete(
        &mut self,
        key: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload.deletes.insert(key.clone(), (old_cs, reason));
        Ok(())
    }

    fn force_delete(&mut self, key: &BookmarkName, reason: BookmarkUpdateReason) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload.force_deletes.insert(key.clone(), reason);
        Ok(())
    }

    fn update_infinitepush(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload
            .infinitepush_sets
            .insert(key.clone(), BookmarkSetData { new_cs, old_cs });
        Ok(())
    }

    fn create_infinitepush(&mut self, key: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
        self.payload.check_if_bookmark_already_used(key)?;
        self.payload
            .infinitepush_creates
            .insert(key.clone(), new_cs);
        Ok(())
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<bool>> {
        self.commit_with_hook(Arc::new(|_ctx, txn| future::ok(txn).boxed()))
    }

    /// commit_with_hook() can be used to have the same transaction to update two different database
    /// tables. `txn_hook()` should apply changes to the transaction.
    fn commit_with_hook(
        self: Box<Self>,
        txn_hook: TransactionHook,
    ) -> BoxFuture<'static, Result<bool>> {
        let Self {
            repo_id,
            ctx,
            payload,
            write_connection,
            ..
        } = *self;

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let commit_fut = conditional_retry_without_delay(
            move |_attempt| {
                write_connection
                    .start_transaction()
                    .compat()
                    .map_err(BookmarkTransactionError::Other)
                    .and_then({
                        cloned!(ctx, txn_hook);
                        move |txn| txn_hook(ctx.clone(), txn)
                    })
                    .and_then({
                        cloned!(payload);
                        move |txn| Self::attempt_write(txn, repo_id, payload)
                    })
            },
            |err, attempt| match err {
                BookmarkTransactionError::RetryableError(_) => {
                    attempt < MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT
                }
                _ => false,
            },
        );

        commit_fut
            .then(|result| match result {
                Ok((transaction, attempts)) => {
                    STATS::bookmarks_update_log_insert_success.add_value(1);
                    STATS::bookmarks_update_log_insert_success_attempt_count
                        .add_value(attempts as i64);
                    transaction
                        .commit()
                        .compat()
                        .and_then(|()| future::ok(true))
                        .left_future()
                }
                Err((BookmarkTransactionError::LogicError, attempts)) => {
                    // Logic error signifies that the transaction was rolled
                    // back, which likely means that bookmark has moved since
                    // our pushrebase finished. We need to retry the pushrebase
                    // Attempt count means one more than the number of `RetryableError`
                    // we hit before seeing this.
                    STATS::bookmarks_insert_logic_error.add_value(1);
                    STATS::bookmarks_insert_logic_error_attempt_count.add_value(attempts as i64);
                    future::ok(false).right_future()
                }
                Err((BookmarkTransactionError::RetryableError(err), attempts)) => {
                    // Attempt count for `RetryableError` should always be equal
                    // to the MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT, and hitting
                    // this error here basically means that this number of attempts
                    // was not enough, or the error was misclassified
                    STATS::bookmarks_insert_retryable_error.add_value(1);
                    STATS::bookmarks_insert_retryable_error_attempt_count
                        .add_value(attempts as i64);
                    future::err(err).right_future()
                }
                Err((BookmarkTransactionError::Other(err), attempts)) => {
                    // `Other` error captures what we consider an "infrastructure"
                    // error, e.g. xdb went down during this transaction.
                    // Attempt count > 1 means the before we hit this error,
                    // we hit `RetryableError` a attempt count - 1 times.
                    STATS::bookmarks_insert_other_error.add_value(1);
                    STATS::bookmarks_insert_other_error_attempt_count.add_value(attempts as i64);
                    future::err(err).right_future()
                }
            })
            .boxed()
    }
}

impl SqlBookmarksTransaction {
    pub async fn update_transaction(
        self,
        transaction: SqlTransaction,
    ) -> Result<TransactionResult> {
        self.ctx
            .perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        match Self::attempt_write(transaction, self.repo_id, self.payload).await {
            Ok(r) => Ok(TransactionResult::Succeeded(r)),
            Err(BookmarkTransactionError::LogicError) => Ok(TransactionResult::Failed),
            Err(BookmarkTransactionError::RetryableError(err)) => Err(err),
            Err(BookmarkTransactionError::Other(err)) => Err(err),
        }
    }
}

#[derive(Clone)]
struct BookmarkSetData {
    new_cs: ChangesetId,
    old_cs: ChangesetId,
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
    use fbinit::FacebookInit;
    use mononoke_types_mocks::{
        changesetid::{ONES_CSID, TWOS_CSID},
        repo::REPO_ZERO,
    };
    use quickcheck::quickcheck;
    use std::collections::HashSet;
    use std::iter::FromIterator;
    use tokio_compat::runtime::Runtime;

    #[fbinit::compat_test]
    async fn test_conditional_retry_without_delay(_fb: FacebookInit) -> Result<()> {
        let fn_to_retry = move |attempt| {
            if attempt < 3 {
                future::err(BookmarkTransactionError::RetryableError(Error::msg(
                    "fails on initial attempts",
                )))
            } else {
                future::ok(())
            }
        };

        let (_res, attempts) =
            conditional_retry_without_delay(fn_to_retry, |_err, attempt| attempt < 4)
                .await
                .expect("should succeed after 3 attempts");
        assert_eq!(attempts, 3);

        let (_err, attempts) =
            conditional_retry_without_delay(fn_to_retry, |_err, attempt| attempt < 1)
                .await
                .expect_err("retries shouldn't have been performed");
        assert_eq!(attempts, 1);

        Ok(())
    }

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
                &BookmarkHgKind::Scratch,
            ),
            (
                &REPO_ZERO,
                &publishing_name,
                &ONES_CSID,
                &BookmarkHgKind::PublishingNotPullDefault,
            ),
            (
                &REPO_ZERO,
                &pull_default_name,
                &ONES_CSID,
                &BookmarkHgKind::PullDefault,
            ),
        ];

        InsertBookmarks::query(&conn, &rows[..]).compat().await?;

        // Create normal over scratch should fail
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create_infinitepush(&publishing_name, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Create scratch over normal should fail
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create(&scratch_name, ONES_CSID, data.clone())?;
        assert!(!txn.commit().await?);

        // Updating publishing with infinite push should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&publishing_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Updating pull default with infinite push should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&pull_default_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Updating publishing with normal should succeed
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&publishing_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(txn.commit().await?);

        // Updating pull default with normal should succeed
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&pull_default_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(txn.commit().await?);

        // Updating scratch with normal should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&scratch_name, TWOS_CSID, ONES_CSID, data.clone())?;
        assert!(!txn.commit().await?);

        // Updating scratch with infinite push should succeed.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&scratch_name, TWOS_CSID, ONES_CSID)?;
        assert!(txn.commit().await?);

        Ok(())
    }

    fn insert_then_query(
        fb: FacebookInit,
        bookmarks: &Vec<(Bookmark, ChangesetId)>,
        query: fn(
            SqlBookmarks,
            ctx: CoreContext,
            &BookmarkPrefix,
            RepositoryId,
            Freshness,
        ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>>,
        freshness: Freshness,
    ) -> HashSet<(Bookmark, ChangesetId)> {
        let mut rt = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(123);

        let store = SqlBookmarks::with_sqlite_in_memory().unwrap();
        let conn = store.write_connection.clone();

        let rows: Vec<_> = bookmarks
            .iter()
            .map(|(bookmark, changeset_id)| {
                (&repo_id, bookmark.name(), changeset_id, bookmark.hg_kind())
            })
            .collect();

        rt.block_on(InsertBookmarks::query(&conn, &rows[..]))
            .expect("insert failed");

        let stream = query(store, ctx, &BookmarkPrefix::empty(), repo_id, freshness)
            .try_collect::<Vec<_>>()
            .compat();

        let res = rt.block_on(stream).expect("query failed");
        HashSet::from_iter(res)
    }

    quickcheck! {
        fn filter_publishing(fb: FacebookInit, bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
                bookmarks.list_publishing_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.publishing()));
            want == have
        }

        fn filter_pull_default(fb: FacebookInit, bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
                bookmarks.list_pull_default_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.pull_default()));
            want == have
        }

        fn filter_all(fb: FacebookInit, bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
                bookmarks.list_all_by_prefix(ctx, prefix, repo_id, freshness, DEFAULT_MAX)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks);
            want == have
        }
    }
}
