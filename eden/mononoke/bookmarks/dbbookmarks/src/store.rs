/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Error, Result};
use bookmarks::{
    Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, BookmarkTransaction,
    BookmarkUpdateLog, BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks, Freshness,
    RawBundleReplayData,
};
use context::{CoreContext, PerfCounterType};
use futures::compat::Future01CompatExt;
use futures::future::{self, BoxFuture, Future, FutureExt, TryFutureExt};
use futures::stream::{self, BoxStream, StreamExt, TryStreamExt};
use mononoke_types::Timestamp;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::queries;
use sql_ext::SqlConnections;
use stats::prelude::*;

use crate::transaction::SqlBookmarksTransaction;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list: timeseries(Rate, Sum),
    list_maybe_stale: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
}

queries! {
    read SelectBookmark(repo_id: RepositoryId, name: BookmarkName) -> (ChangesetId) {
        "SELECT changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
         LIMIT 1"
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

    read CountFurtherBookmarkLogEntriesWithoutReason(min_id: u64, repo_id: RepositoryId, reason: BookmarkUpdateReason) -> (u64) {
        "SELECT COUNT(*)
        FROM bookmarks_update_log
        WHERE id > {min_id} AND repo_id = {repo_id} AND NOT reason = {reason}"
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

}

#[derive(Clone)]
pub struct SqlBookmarks {
    repo_id: RepositoryId,
    pub(crate) connections: SqlConnections,
}

impl SqlBookmarks {
    pub(crate) fn new(repo_id: RepositoryId, connections: SqlConnections) -> Self {
        Self {
            repo_id,
            connections,
        }
    }
}

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
                &self.connections.read_connection
            }
            Freshness::MostRecent => {
                STATS::list.add_value(1);
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                &self.connections.read_master_connection
            }
        };

        if prefix.is_empty() {
            match pagination {
                BookmarkPagination::FromStart => {
                    query_to_stream(SelectAll::query(&conn, &self.repo_id, &limit, kinds).compat())
                }
                BookmarkPagination::After(ref after) => query_to_stream(
                    SelectAllAfter::query(&conn, &self.repo_id, after, &limit, kinds).compat(),
                ),
            }
        } else {
            let prefix_like_pattern = prefix.to_escaped_sql_like_pattern();
            match pagination {
                BookmarkPagination::FromStart => query_to_stream(
                    SelectByPrefix::query(
                        &conn,
                        &self.repo_id,
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
                        &self.repo_id,
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
    ) -> BoxFuture<'static, Result<Option<ChangesetId>>> {
        STATS::get_bookmark.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        SelectBookmark::query(
            &self.connections.read_master_connection,
            &self.repo_id,
            &name,
        )
        .compat()
        .map_ok(|rows| rows.into_iter().next().map(|row| row.0))
        .boxed()
    }

    fn create_transaction(&self, ctx: CoreContext) -> Box<dyn BookmarkTransaction> {
        Box::new(SqlBookmarksTransaction::new(
            ctx,
            self.connections.write_connection.clone(),
            self.repo_id.clone(),
        ))
    }
}

impl BookmarkUpdateLog for SqlBookmarks {
    fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            &self.connections.read_master_connection
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            &self.connections.read_connection
        };

        match offset {
            Some(offset) => SelectBookmarkLogsWithOffset::query(
                &connection,
                &self.repo_id,
                &name,
                &max_rec,
                &offset,
            )
            .compat()
            .map_ok(|rows| stream::iter(rows.into_iter().map(Ok)))
            .try_flatten_stream()
            .boxed(),
            None => SelectBookmarkLogs::query(&connection, &self.repo_id, &name, &max_rec)
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
        maybe_exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<'static, Result<u64>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let query = match maybe_exclude_reason {
            Some(ref r) => CountFurtherBookmarkLogEntriesWithoutReason::query(
                &self.connections.read_connection,
                &id,
                &self.repo_id,
                &r,
            )
            .compat()
            .boxed(),
            None => CountFurtherBookmarkLogEntries::query(
                &self.connections.read_connection,
                &id,
                &self.repo_id,
            )
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
    ) -> BoxFuture<'static, Result<Vec<(BookmarkUpdateReason, u64)>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        CountFurtherBookmarkLogEntriesByReason::query(
            &self.connections.read_connection,
            &id,
            &self.repo_id,
        )
        .compat()
        .map_ok(|entries| entries.into_iter().collect())
        .boxed()
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<'static, Result<Option<u64>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        SkipOverBookmarkLogEntriesWithReason::query(
            &self.connections.read_connection,
            &id,
            &self.repo_id,
            &reason,
        )
        .compat()
        .map_ok(|entries| entries.first().map(|entry| entry.0))
        .boxed()
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        ReadNextBookmarkLogEntries::query(
            &self.connections.read_connection,
            &id,
            &self.repo_id,
            &limit,
        )
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
                    commit_timestamps_json,
                ) = entry;
                let bundle_replay_data =
                    RawBundleReplayData::maybe_new(bundle_handle, commit_timestamps_json)?;
                Ok(BookmarkUpdateLogEntry {
                    id,
                    repo_id,
                    bookmark_name: name,
                    to_changeset_id: to_cs_id,
                    from_changeset_id: from_cs_id,
                    reason,
                    timestamp,
                    bundle_replay_data,
                })
            })
        })
        .try_flatten_stream()
        .boxed()
    }

    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            &self.connections.read_master_connection
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            &self.connections.read_connection
        };

        ReadNextBookmarkLogEntries::query(&connection, &id, &self.repo_id, &limit)
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
                        commit_timestamps_json,
                    ) = entry;
                    let bundle_replay_data =
                        RawBundleReplayData::maybe_new(bundle_handle, commit_timestamps_json)?;
                    Ok(BookmarkUpdateLogEntry {
                        id,
                        repo_id,
                        bookmark_name: name,
                        to_changeset_id: to_cs_id,
                        from_changeset_id: from_cs_id,
                        reason,
                        timestamp,
                        bundle_replay_data,
                    })
                })
            })
            .try_flatten_stream()
            .boxed()
    }
}
