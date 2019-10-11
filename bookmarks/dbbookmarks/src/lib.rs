/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use bookmarks::{
    Bookmark, BookmarkHgKind, BookmarkName, BookmarkPrefix, BookmarkUpdateLogEntry,
    BookmarkUpdateReason, Bookmarks, BundleReplayData, Freshness, Transaction,
};
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use failure_ext::{bail_msg, err_msg, format_err, Error, Result};
use futures::{
    future::{self, loop_fn, Loop},
    stream, Future, IntoFuture, Stream,
};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use mononoke_types::Timestamp;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection, Transaction as SqlTransaction};
pub use sql_ext::SqlConstructors;
use stats::{define_stats, Timeseries};
use std::collections::HashMap;

const DEFAULT_MAX: u64 = std::u64::MAX;
const MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT: usize = 5;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list_all_by_prefix: timeseries(RATE, SUM),
    list_all_by_prefix_maybe_stale: timeseries(RATE, SUM),
    list_pull_default_by_prefix: timeseries(RATE, SUM),
    list_pull_default_by_prefix_maybe_stale: timeseries(RATE, SUM),
    list_publishing_by_prefix: timeseries(RATE, SUM),
    list_publishing_by_prefix_maybe_stale: timeseries(RATE, SUM),
    get_bookmark: timeseries(RATE, SUM),
    bookmarks_update_log_insert_success: timeseries(RATE, SUM),
    bookmarks_update_log_insert_success_attempt_count: timeseries(RATE, AVG, SUM),
    bookmarks_insert_retryable_error: timeseries(RATE, SUM),
    bookmarks_insert_retryable_error_attempt_count: timeseries(RATE, AVG, SUM),
    bookmarks_insert_logic_error: timeseries(RATE, SUM),
    bookmarks_insert_logic_error_attempt_count: timeseries(RATE, AVG, SUM),
    bookmarks_insert_other_error: timeseries(RATE, SUM),
    bookmarks_insert_other_error_attempt_count: timeseries(RATE, AVG, SUM),
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

impl SqlConstructors for SqlBookmarks {
    const LABEL: &'static str = "bookmarks";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-bookmarks.sql")
    }
}

fn query_to_stream<F>(v: F, max: u64) -> BoxStream<(Bookmark, ChangesetId), Error>
where
    F: Future<Item = Vec<(BookmarkName, BookmarkHgKind, ChangesetId)>, Error = Error>
        + Send
        + 'static,
{
    v.map(move |rows| {
        if rows.len() as u64 >= max {
            let message = format_err!(
                "Bookmark query was truncated after {} results, use a more specific prefix search.",
                max
            );
            future::err(message).into_stream().left_stream()
        } else {
            stream::iter_ok(rows).right_stream()
        }
    })
    .flatten_stream()
    .map(|row| {
        let (name, hg_kind, changeset_id) = row;
        (Bookmark::new(name, hg_kind), changeset_id)
    })
    .boxify()
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
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        let conn = match freshness {
            Freshness::MostRecent => &self.read_master_connection,
            Freshness::MaybeStale => &self.read_connection,
        };

        let query = if prefix.is_empty() {
            SelectAll::query(&conn, &repo_id, &max, kinds).left_future()
        } else {
            SelectByPrefix::query(&conn, &repo_id, &prefix, &max, kinds).right_future()
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
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
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
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
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
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
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
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        SelectBookmark::query(&self.read_master_connection, &repo_id, &name)
            .map(|rows| rows.into_iter().next().map(|row| row.0))
            .boxify()
    }

    fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        repo_id: RepositoryId,
        max_rec: u32,
    ) -> BoxStream<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        SelectBookmarkLogs::query(&self.read_master_connection, &repo_id, &name, &max_rec)
            .map(|rows| stream::iter_ok(rows))
            .flatten_stream()
            .boxify()
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
    ) -> BoxFuture<u64, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let query = match maybe_exclude_reason {
            Some(ref r) => CountFurtherBookmarkLogEntriesWithoutReason::query(
                &self.read_connection,
                &id,
                &repoid,
                &r,
            )
            .boxify(),
            None => {
                CountFurtherBookmarkLogEntries::query(&self.read_connection, &id, &repoid).boxify()
            }
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
                        future::err(err_msg(msg))
                    }
                }
            })
            .boxify()
    }

    fn count_further_bookmark_log_entries_by_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<Vec<(BookmarkUpdateReason, u64)>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        CountFurtherBookmarkLogEntriesByReason::query(&self.read_connection, &id, &repoid)
            .map(|entries| entries.into_iter().collect())
            .boxify()
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<Option<u64>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        SkipOverBookmarkLogEntriesWithReason::query(&self.read_connection, &id, &repoid, &reason)
            .map(|entries| entries.first().map(|entry| entry.0))
            .boxify()
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        ReadNextBookmarkLogEntries::query(&self.read_connection, &id, &repoid, &limit)
            .map(|entries| {
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
                stream::iter_ok(homogenous_entries).and_then(|entry| {
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
            .flatten_stream()
            .boxify()
    }

    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        ReadNextBookmarkLogEntries::query(&self.read_connection, &id, &repoid, &limit)
            .map(|entries| {
                stream::iter_ok(entries.into_iter()).and_then(|entry| {
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
            .flatten_stream()
            .boxify()
    }
}

#[derive(Debug)]
enum BookmarkTransactionError {
    // The transaction modifying bookmarks tables should be retried
    RetryableError(Error),
    // Transacton was rolled back, we consider this a logic error,
    // which may prompt retry higher in the stack. This can happen
    // for example if some other bookmark update won the race and
    // the entire pushrebase needs to be retried
    LogicError,
    // Something unexpected went wrong
    Other(Error),
}

type RetryAttempt = usize;

fn conditional_retry_without_delay<V, Fut, RetryableFunc, DecisionFunc>(
    func: RetryableFunc,
    should_retry: DecisionFunc,
) -> impl Future<Item = (V, RetryAttempt), Error = (BookmarkTransactionError, RetryAttempt)>
where
    V: Send + 'static,
    Fut: Future<Item = V, Error = BookmarkTransactionError>,
    RetryableFunc: Fn(RetryAttempt) -> Fut + Send + 'static,
    DecisionFunc: Fn(&BookmarkTransactionError, RetryAttempt) -> bool + Send + 'static + Clone,
{
    loop_fn(1, move |attempt| {
        func(attempt)
            .and_then(move |res| Ok(Loop::Break(Ok((res, attempt)))))
            .or_else({
                cloned!(should_retry);
                move |err| {
                    if should_retry(&err, attempt) {
                        Ok(Loop::Continue(attempt + 1)).into_future()
                    } else {
                        Ok(Loop::Break(Err((err, attempt)))).into_future()
                    }
                }
            })
    })
    .flatten()
}

struct SqlBookmarksTransaction {
    write_connection: Connection,
    repo_id: RepositoryId,
    force_sets: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    creates: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    sets: HashMap<BookmarkName, (BookmarkSetData, BookmarkUpdateReason)>,
    force_deletes: HashMap<BookmarkName, BookmarkUpdateReason>,
    deletes: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    infinitepush_sets: HashMap<BookmarkName, BookmarkSetData>,
    infinitepush_creates: HashMap<BookmarkName, ChangesetId>,
    ctx: CoreContext,
}

impl SqlBookmarksTransaction {
    fn new(ctx: CoreContext, write_connection: Connection, repo_id: RepositoryId) -> Self {
        Self {
            write_connection,
            repo_id,
            force_sets: HashMap::new(),
            creates: HashMap::new(),
            sets: HashMap::new(),
            force_deletes: HashMap::new(),
            deletes: HashMap::new(),
            infinitepush_sets: HashMap::new(),
            infinitepush_creates: HashMap::new(),
            ctx,
        }
    }

    fn check_if_bookmark_already_used(&self, key: &BookmarkName) -> Result<()> {
        if self.creates.contains_key(key)
            || self.force_sets.contains_key(key)
            || self.sets.contains_key(key)
            || self.force_deletes.contains_key(key)
            || self.deletes.contains_key(key)
            || self.infinitepush_sets.contains_key(key)
            || self.infinitepush_creates.contains_key(key)
        {
            bail_msg!("{} bookmark was already used", key);
        }
        Ok(())
    }

    fn log_bundle_replay_data(
        id: u64,
        reason: BookmarkUpdateReason,
        sql_transaction: SqlTransaction,
    ) -> impl Future<Item = SqlTransaction, Error = Error> {
        use BookmarkUpdateReason::*;
        match reason {
            Pushrebase {
                bundle_replay_data: Some(bundle_replay_data),
            }
            | Push {
                bundle_replay_data: Some(bundle_replay_data),
            }
            | TestMove {
                bundle_replay_data: Some(bundle_replay_data),
            } => {
                let BundleReplayData {
                    bundle_handle,
                    commit_timestamps,
                } = bundle_replay_data;
                let commit_timestamps = try_boxfuture!(serde_json::to_string(&commit_timestamps));

                AddBundleReplayData::query_with_transaction(
                    sql_transaction,
                    &[(&id, &bundle_handle, &commit_timestamps)],
                )
                .map(move |(sql_transaction, _)| sql_transaction)
                .boxify()
            }
            _ => future::ok(sql_transaction).boxify(),
        }
    }

    fn find_next_update_log_id(
        sql_transaction: SqlTransaction,
    ) -> impl Future<Item = (SqlTransaction, u64), Error = Error> {
        return FindMaxBookmarkLogId::query_with_transaction(sql_transaction).and_then(
            |(sql_transaction, max_id_entries)| {
                let next_id = 1 + match &max_id_entries[..] {
                    [(None,)] => 0,
                    [(Some(max_existing),)] => *max_existing,
                    // TODO (ikostia): consider panicking here
                    _ => {
                        return future::err(format_err!(
                            "Should be impossible. FindMaxBookmarkLogId returned not a single entry: {:?}",
                            max_id_entries
                        ))
                    }
                };
                future::ok((sql_transaction, next_id))
            },
        );
    }

    fn log_bookmark_moves(
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
    ) -> impl Future<Item = SqlTransaction, Error = Error> {
        Self::find_next_update_log_id(sql_transaction).and_then(
            move |(sql_transaction, next_id)| {
                loop_fn(
                    (moves.into_iter(), next_id, sql_transaction),
                    move |(mut moves, next_id, sql_transaction)| match moves.next() {
                        Some((bookmark, (from_changeset_id, to_changeset_id, reason))) => {
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
                            AddBookmarkLog::query_with_transaction(sql_transaction, &row[..])
                                .and_then(move |(sql_transaction, _query_result)| {
                                    Self::log_bundle_replay_data(next_id, reason, sql_transaction)
                                        .map(move |sql_transaction| {
                                            (moves, next_id + 1, sql_transaction)
                                        })
                                })
                                .map(Loop::Continue)
                                .left_future()
                        }
                        None => future::ok(Loop::Break(sql_transaction)).right_future(),
                    },
                )
            },
        )
    }

    fn attempt_commit(
        write_connection: Connection,
        repo_id: RepositoryId,
        force_sets: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
        creates: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
        sets: HashMap<BookmarkName, (BookmarkSetData, BookmarkUpdateReason)>,
        force_deletes: HashMap<BookmarkName, BookmarkUpdateReason>,
        deletes: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
        infinitepush_sets: HashMap<BookmarkName, BookmarkSetData>,
        infinitepush_creates: HashMap<BookmarkName, ChangesetId>,
        log_rows: HashMap<
            BookmarkName,
            (
                Option<ChangesetId>,
                Option<ChangesetId>,
                BookmarkUpdateReason,
            ),
        >,
    ) -> BoxFuture<SqlTransaction, BookmarkTransactionError> {
        // NOTE: Infinitepush updates do *not* go into log_rows. This is because the
        // BookmarkUpdateLog is currently used for replays to Mercurial, and those updates should
        // not be replayed (for Infinitepush, those updates are actually dispatched by the client
        // to both destination).

        write_connection
            .start_transaction()
            .map_err(BookmarkTransactionError::Other)
            .and_then(move |transaction| {
                let force_set: Vec<_> = force_sets.clone().into_iter().collect();
                let mut ref_rows = Vec::new();
                for idx in 0..force_set.len() {
                    let (ref to_changeset_id, _) = force_set[idx].1;
                    ref_rows.push((&repo_id, &force_set[idx].0, to_changeset_id));
                }

                ReplaceBookmarks::query_with_transaction(transaction, &ref_rows[..])
                    .map_err(BookmarkTransactionError::Other)
            })
            .and_then(move |(transaction, _)| {
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
                InsertBookmarks::query_with_transaction(transaction, &ref_rows[..]).then(
                    move |res| match res {
                        Err(err) => Err(BookmarkTransactionError::Other(err)),
                        Ok((transaction, result)) => {
                            if result.affected_rows() == rows_to_insert {
                                Ok(transaction)
                            } else {
                                Err(BookmarkTransactionError::LogicError)
                            }
                        }
                    },
                )
            })
            .and_then(move |transaction| {
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

                loop_fn(
                    (transaction, updates_iter),
                    move |(transaction, mut updates)| match updates.next() {
                        Some((
                            ref name,
                            BookmarkSetData {
                                ref new_cs,
                                ref old_cs,
                            },
                            ref _kinds,
                        )) if new_cs == old_cs => {
                            // no-op update. If bookmark points to a correct update then
                            // let's continue the transaction, otherwise revert it
                            SelectBookmark::query_with_transaction(transaction, &repo_id, &name)
                                .then({
                                    let new_cs = new_cs.clone();
                                    move |res| match res {
                                        Err(err) => Err(BookmarkTransactionError::Other(err)),
                                        Ok((transaction, result)) => {
                                            if result.get(0).map(|b| b.0) == Some(new_cs) {
                                                Ok((transaction, updates))
                                            } else {
                                                Err(BookmarkTransactionError::LogicError)
                                            }
                                        }
                                    }
                                })
                                .map(Loop::Continue)
                                .boxify()
                        }
                        Some((name, BookmarkSetData { new_cs, old_cs }, kinds)) => {
                            UpdateBookmark::query_with_transaction(
                                transaction,
                                &repo_id,
                                &name,
                                &old_cs,
                                &new_cs,
                                &kinds[..],
                            )
                            .then(move |res| match res {
                                Err(err) => Err(BookmarkTransactionError::Other(err)),
                                Ok((transaction, result)) => {
                                    if result.affected_rows() == 1 {
                                        Ok((transaction, updates))
                                    } else {
                                        Err(BookmarkTransactionError::LogicError)
                                    }
                                }
                            })
                            .map(Loop::Continue)
                            .boxify()
                        }
                        None => Ok(Loop::Break(transaction)).into_future().boxify(),
                    },
                )
            })
            .and_then(move |transaction| {
                loop_fn(
                    (transaction, force_deletes.into_iter()),
                    move |(transaction, mut deletes)| match deletes.next() {
                        Some((name, _reason)) => {
                            DeleteBookmark::query_with_transaction(transaction, &repo_id, &name)
                                .then(move |res| match res {
                                    Err(err) => Err(BookmarkTransactionError::Other(err)),
                                    Ok((transaction, _)) => Ok((transaction, deletes)),
                                })
                                .map(Loop::Continue)
                                .left_future()
                        }
                        None => Ok(Loop::Break(transaction)).into_future().right_future(),
                    },
                )
            })
            .and_then(move |transaction| {
                loop_fn(
                    (transaction, deletes.into_iter()),
                    move |(transaction, mut deletes)| match deletes.next() {
                        Some((name, (old_cs, _reason))) => {
                            DeleteBookmarkIf::query_with_transaction(
                                transaction,
                                &repo_id,
                                &name,
                                &old_cs,
                            )
                            .then(move |res| match res {
                                Err(err) => Err(BookmarkTransactionError::Other(err)),
                                Ok((transaction, result)) => {
                                    if result.affected_rows() == 1 {
                                        Ok((transaction, deletes))
                                    } else {
                                        Err(BookmarkTransactionError::LogicError)
                                    }
                                }
                            })
                            .map(Loop::Continue)
                            .left_future()
                        }
                        None => Ok(Loop::Break(transaction)).into_future().right_future(),
                    },
                )
            })
            .and_then(move |transaction| {
                Self::log_bookmark_moves(repo_id, Timestamp::now(), log_rows, transaction)
                    .map_err(BookmarkTransactionError::RetryableError)
            })
            .boxify()
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
        self.check_if_bookmark_already_used(key)?;
        self.sets
            .insert(key.clone(), (BookmarkSetData { new_cs, old_cs }, reason));
        Ok(())
    }

    fn create(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.creates.insert(key.clone(), (new_cs, reason));
        Ok(())
    }

    fn force_set(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_sets.insert(key.clone(), (new_cs, reason));
        Ok(())
    }

    fn delete(
        &mut self,
        key: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.deletes.insert(key.clone(), (old_cs, reason));
        Ok(())
    }

    fn force_delete(&mut self, key: &BookmarkName, reason: BookmarkUpdateReason) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_deletes.insert(key.clone(), reason);
        Ok(())
    }

    fn update_infinitepush(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.infinitepush_sets
            .insert(key.clone(), BookmarkSetData { new_cs, old_cs });
        Ok(())
    }

    fn create_infinitepush(&mut self, key: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.infinitepush_creates.insert(key.clone(), new_cs);
        Ok(())
    }

    fn commit(self: Box<Self>) -> BoxFuture<bool, Error> {
        let this = *self;

        let Self {
            write_connection,
            repo_id,
            force_sets,
            creates,
            sets,
            force_deletes,
            deletes,
            infinitepush_sets,
            infinitepush_creates,
            ctx,
        } = this;
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let mut log_rows: HashMap<_, (Option<ChangesetId>, Option<ChangesetId>, _)> =
            HashMap::new();
        for (bookmark, (to_cs_id, reason)) in force_sets.clone() {
            log_rows.insert(bookmark, (None, Some(to_cs_id), reason));
        }

        for (bookmark, (to_cs_id, reason)) in creates.clone() {
            log_rows.insert(bookmark, (None, Some(to_cs_id), reason));
        }

        for (bookmark, (bookmark_set_data, reason)) in sets.clone() {
            let from_cs_id = bookmark_set_data.old_cs;
            let to_cs_id = bookmark_set_data.new_cs;
            log_rows.insert(bookmark, (Some(from_cs_id), Some(to_cs_id), reason));
        }

        for (bookmark, reason) in force_deletes.clone() {
            log_rows.insert(bookmark, (None, None, reason));
        }

        for (bookmark, (from_cs_id, reason)) in deletes.clone() {
            log_rows.insert(bookmark, (Some(from_cs_id), None, reason));
        }

        let commit_fut = conditional_retry_without_delay(
            move |_attempt| {
                Self::attempt_commit(
                    write_connection.clone(),
                    repo_id,
                    force_sets.clone(),
                    creates.clone(),
                    sets.clone(),
                    force_deletes.clone(),
                    deletes.clone(),
                    infinitepush_sets.clone(),
                    infinitepush_creates.clone(),
                    log_rows.clone(),
                )
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
                    transaction.commit().and_then(|()| Ok(true)).left_future()
                }
                Err((BookmarkTransactionError::LogicError, attempts)) => {
                    // Logic error signifies that the transaction was rolled
                    // back, which likely means that bookmark has moved since
                    // our pushrebase finished. We need to retry the pushrebase
                    // Attempt count means one more than the number of `RetryableError`
                    // we hit before seeing this.
                    STATS::bookmarks_insert_logic_error.add_value(1);
                    STATS::bookmarks_insert_logic_error_attempt_count.add_value(attempts as i64);
                    Ok(false).into_future().right_future()
                }
                Err((BookmarkTransactionError::RetryableError(err), attempts)) => {
                    // Attempt count for `RetryableError` should always be equal
                    // to the MAX_BOOKMARK_TRANSACTION_ATTEMPT_COUNT, and hitting
                    // this error here basically means that this number of attempts
                    // was not enough, or the error was misclassified
                    STATS::bookmarks_insert_retryable_error.add_value(1);
                    STATS::bookmarks_insert_retryable_error_attempt_count
                        .add_value(attempts as i64);
                    Err(err).into_future().right_future()
                }
                Err((BookmarkTransactionError::Other(err), attempts)) => {
                    // `Other` error captures what we consider an "infrastructure"
                    // error, e.g. xdb went down during this transaction.
                    // Attempt count > 1 means the before we hit this error,
                    // we hit `RetryableError` a attempt count - 1 times.
                    STATS::bookmarks_insert_other_error.add_value(1);
                    STATS::bookmarks_insert_other_error_attempt_count.add_value(attempts as i64);
                    Err(err).into_future().right_future()
                }
            })
            .boxify()
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
        _ => Err(err_msg("inconsistent replay data")),
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
    use tokio::runtime::Runtime;

    #[test]
    fn test_conditional_retry_without_delay() {
        let mut rt = Runtime::new().unwrap();

        let fn_to_retry = move |attempt| {
            if attempt < 3 {
                future::err(BookmarkTransactionError::RetryableError(err_msg(
                    "fails on initial attempts",
                )))
            } else {
                future::ok(())
            }
        };

        let should_succeed =
            conditional_retry_without_delay(fn_to_retry, |_err, attempt| attempt < 4);
        let (_res, attempts) = rt
            .block_on(should_succeed)
            .expect("retries failed, but should've succeeded");
        assert_eq!(attempts, 3);

        let should_fail = conditional_retry_without_delay(fn_to_retry, |_err, attempt| attempt < 1);
        let (_err, attempts) = rt
            .block_on(should_fail)
            .expect_err("retries shouldn't have been performed");
        assert_eq!(attempts, 1);
    }

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book.to_string()).unwrap()
    }

    #[fbinit::test]
    fn test_update_kind_compatibility(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();

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

        rt.block_on(InsertBookmarks::query(&conn, &rows[..]))
            .expect("insert failed");

        // Create normal over scratch should fail
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create_infinitepush(&publishing_name, ONES_CSID)
            .unwrap();
        assert!(!txn.commit().wait().unwrap());

        // Create scratch over normal should fail
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.create(&scratch_name, ONES_CSID, data.clone()).unwrap();
        assert!(!txn.commit().wait().unwrap());

        // Updating publishing with infinite push should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&publishing_name, TWOS_CSID, ONES_CSID)
            .unwrap();
        assert!(!txn.commit().wait().unwrap());

        // Updating pull default with infinite push should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&pull_default_name, TWOS_CSID, ONES_CSID)
            .unwrap();
        assert!(!txn.commit().wait().unwrap());

        // Updating publishing with normal should succeed
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&publishing_name, TWOS_CSID, ONES_CSID, data.clone())
            .unwrap();
        assert!(txn.commit().wait().unwrap());

        // Updating pull default with normal should succeed
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&pull_default_name, TWOS_CSID, ONES_CSID, data.clone())
            .unwrap();
        assert!(txn.commit().wait().unwrap());

        // Updating scratch with normal should fail.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update(&scratch_name, TWOS_CSID, ONES_CSID, data.clone())
            .unwrap();
        assert!(!txn.commit().wait().unwrap());

        // Updating scratch with infinite push should succeed.
        let mut txn = store.create_transaction(ctx.clone(), REPO_ZERO);
        txn.update_infinitepush(&scratch_name, TWOS_CSID, ONES_CSID)
            .unwrap();
        assert!(txn.commit().wait().unwrap());
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
        ) -> BoxStream<(Bookmark, ChangesetId), Error>,
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

        let stream = query(store, ctx, &BookmarkPrefix::empty(), repo_id, freshness).collect();

        let res = rt.block_on(stream).expect("query failed");
        HashSet::from_iter(res)
    }

    quickcheck! {
        fn filter_publishing(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_publishing_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.publishing()));
            want == have
        }

        fn filter_pull_default(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_pull_default_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.pull_default()));
            want == have
        }

        fn filter_all(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            fn query(bookmarks: SqlBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_all_by_prefix(ctx, prefix, repo_id, freshness, DEFAULT_MAX)
            }

            let have = insert_then_query(fb, &bookmarks, query, freshness);
            let want = HashSet::from_iter(bookmarks);
            want == have
        }
    }
}
