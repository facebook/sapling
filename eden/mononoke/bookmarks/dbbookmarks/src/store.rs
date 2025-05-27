/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use bookmarks::Bookmark;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksSubscription;
use bookmarks::Freshness;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use context::SessionClass;
use futures::future::BoxFuture;
use futures::future::Future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_watchdog::WatchdogExt;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use rand::Rng;
use sql::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;
use stats::prelude::*;

use crate::subscription::SqlBookmarksSubscription;
use crate::transaction::SqlBookmarksTransaction;

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list: timeseries(Rate, Sum),
    list_maybe_stale: timeseries(Rate, Sum),
    list_wbc: timeseries(Rate, Sum),
    list_maybe_stale_wbc: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
}

mononoke_queries! {
    pub(crate) read SelectBookmark(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory) -> (ChangesetId, Option<u64>) {
        "SELECT changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
         LIMIT 1"
    }

    read SelectAll(
        repo_id: RepositoryId,
        limit: u64,
        >list kinds: BookmarkKind
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    pub(crate) read SelectAllUnordered(
        repo_id: RepositoryId,
        limit: u64,
        tok: i32,
        >list kinds: BookmarkKind
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>, i32) {
        mysql("SELECT name, category, hg_kind, changeset_id, log_id, {tok}
         FROM bookmarks
         FORCE INDEX(repo_id_hg_kind_category)
         WHERE repo_id = {repo_id}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         LIMIT {limit}")
        sqlite("SELECT name, category, hg_kind, changeset_id, log_id, {tok}
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         LIMIT {limit}")
    }

    read SelectAllAfter(
        repo_id: RepositoryId,
        after: BookmarkName,
        limit: u64,
        >list kinds: BookmarkKind
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name > {after}
           AND category IN {categories}
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
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name LIKE {prefix_like_pattern} ESCAPE {escape_character}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    read SelectByPrefixUnordered(
        repo_id: RepositoryId,
        prefix_like_pattern: String,
        escape_character: &str,
        limit: u64,
        >list kinds: BookmarkKind
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name LIKE {prefix_like_pattern} ESCAPE {escape_character}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         LIMIT {limit}"
    }

    read SelectByPrefixAfter(
        repo_id: RepositoryId,
        prefix_like_pattern: String,
        escape_character: &str,
        after: BookmarkName,
        limit: u64,
        >list kinds:BookmarkKind
        >list categories: BookmarkCategory
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, Option<u64>) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name LIKE {prefix_like_pattern} ESCAPE {escape_character}
           AND name > {after}
           AND category IN {categories}
           AND hg_kind IN {kinds}
         ORDER BY name ASC
         LIMIT {limit}"
    }

    read SelectAfterLogId(
        repo_id: RepositoryId,
        log_id: u64,
    ) -> (BookmarkName, BookmarkCategory, BookmarkKind, ChangesetId, u64) {
        "SELECT name, category, hg_kind, changeset_id, log_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND log_id IS NOT NULL
           AND log_id > {log_id}"
    }

    read ReadNextBookmarkLogEntries(min_id: u64, repo_id: RepositoryId, limit: u64) -> (
        i64, RepositoryId, BookmarkName, BookmarkCategory, Option<ChangesetId>, Option<ChangesetId>,
        BookmarkUpdateReason, Timestamp
    ) {
        "SELECT id, repo_id, name, category, to_changeset_id, from_changeset_id, reason, timestamp
         FROM bookmarks_update_log
         WHERE id > {min_id} AND repo_id = {repo_id}
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

    read SelectBookmarkLogs(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory, max_records: u32, tok: i32) -> (
        u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp, i32
    ) {
        "SELECT id, to_changeset_id, reason, timestamp, {tok}
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
         ORDER BY id DESC
         LIMIT {max_records}"
    }

    read SelectBookmarkLogsWithTsInRange(
        repo_id: RepositoryId,
        name: BookmarkName,
        category: BookmarkCategory,
        max_records: u32,
        min_ts: Timestamp,
        max_ts: Timestamp
    ) -> (
        u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp
    ) {
        "SELECT id, to_changeset_id, reason, timestamp
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
           AND timestamp >= {min_ts}
           AND timestamp <= {max_ts}
         ORDER BY id DESC
         LIMIT {max_records}"
    }

    read SelectBookmarkLogsWithOffset(repo_id: RepositoryId, name: BookmarkName, category: BookmarkCategory, max_records: u32, offset: u32, tok: i32) -> (
        u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp, i32
    ) {
        "SELECT id, to_changeset_id, reason, timestamp, {tok}
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND category = {category}
         ORDER BY id DESC
         LIMIT {max_records}
         OFFSET {offset}"
    }

    pub(crate) read GetLargestLogId(repo_id: RepositoryId) -> (Option<u64>) {
        "SELECT MAX(id)
         FROM bookmarks_update_log
         WHERE repo_id = {repo_id}"
    }
}

#[facet::facet]
#[derive(Clone)]
pub struct SqlBookmarks {
    pub(crate) repo_id: RepositoryId,
    pub(crate) connections: SqlConnections,
}

impl SqlBookmarks {
    pub(crate) fn new(repo_id: RepositoryId, connections: SqlConnections) -> Self {
        Self {
            repo_id,
            connections,
        }
    }

    pub fn connection(&self, ctx: &CoreContext, freshness: Freshness) -> &Connection {
        match freshness {
            Freshness::MaybeStale => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
                &self.connections.read_connection
            }
            Freshness::MostRecent => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                &self.connections.read_master_connection
            }
        }
    }

    pub fn list_raw(
        &self,
        ctx: &CoreContext,
        freshness: Freshness,
        prefix: &BookmarkPrefix,
        categories: &[BookmarkCategory],
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<(BookmarkKey, BookmarkKind, ChangesetId, Option<u64>)>>> + use<>
    {
        let is_wbc = matches!(
            ctx.session().session_class(),
            SessionClass::WarmBookmarksCache
        );

        let conn = match freshness {
            Freshness::MaybeStale => {
                STATS::list_maybe_stale.add_value(1);
                if is_wbc {
                    STATS::list_maybe_stale_wbc.add_value(1);
                }
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
                self.connections.read_connection.clone()
            }
            Freshness::MostRecent => {
                STATS::list.add_value(1);
                if is_wbc {
                    STATS::list_wbc.add_value(2);
                }
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                self.connections.read_master_connection.clone()
            }
        };

        cloned!(pagination, prefix, ctx, self.repo_id);
        let kinds: Vec<BookmarkKind> = kinds.to_vec();
        let categories: Vec<_> = categories.to_vec();

        async move {
            let cri = ctx.client_request_info();
            let rows = if prefix.is_empty() {
                match pagination {
                    BookmarkPagination::FromStart => {
                        // Sorting is only useful for pagination. If the query returns all bookmark
                        // names, then skip the sorting.
                        if limit == u64::MAX {
                            let tok: i32 = rand::thread_rng().r#gen();
                            SelectAllUnordered::maybe_traced_query(
                                &conn,
                                cri,
                                &repo_id,
                                &limit,
                                &tok,
                                &kinds,
                                &categories,
                            )
                            .await?
                            .into_iter()
                            .map(|(name, category, kind, cs_id, log_id, _)| {
                                (
                                    BookmarkKey::with_name_and_category(name, category),
                                    kind,
                                    cs_id,
                                    log_id,
                                )
                            })
                            .collect()
                        } else {
                            SelectAll::maybe_traced_query(
                                &conn,
                                cri,
                                &repo_id,
                                &limit,
                                &kinds,
                                &categories,
                            )
                            .await?
                            .into_iter()
                            .map(|(name, category, kind, cs_id, log_id)| {
                                (
                                    BookmarkKey::with_name_and_category(name, category),
                                    kind,
                                    cs_id,
                                    log_id,
                                )
                            })
                            .collect()
                        }
                    }
                    BookmarkPagination::After(after) => SelectAllAfter::maybe_traced_query(
                        &conn,
                        cri,
                        &repo_id,
                        &after,
                        &limit,
                        &kinds,
                        &categories,
                    )
                    .await?
                    .into_iter()
                    .map(|(name, category, kind, cs_id, log_id)| {
                        (
                            BookmarkKey::with_name_and_category(name, category),
                            kind,
                            cs_id,
                            log_id,
                        )
                    })
                    .collect(),
                }
            } else {
                let prefix_like_pattern = prefix.to_escaped_sql_like_pattern();
                match pagination {
                    BookmarkPagination::FromStart => {
                        if limit == u64::MAX {
                            SelectByPrefixUnordered::maybe_traced_query(
                                &conn,
                                cri,
                                &repo_id,
                                &prefix_like_pattern,
                                &"\\",
                                &limit,
                                &kinds,
                                &categories,
                            )
                            .await?
                            .into_iter()
                            .map(|(name, category, kind, cs_id, log_id)| {
                                (
                                    BookmarkKey::with_name_and_category(name, category),
                                    kind,
                                    cs_id,
                                    log_id,
                                )
                            })
                            .collect()
                        } else {
                            SelectByPrefix::maybe_traced_query(
                                &conn,
                                cri,
                                &repo_id,
                                &prefix_like_pattern,
                                &"\\",
                                &limit,
                                &kinds,
                                &categories,
                            )
                            .await?
                            .into_iter()
                            .map(|(name, category, kind, cs_id, log_id)| {
                                (
                                    BookmarkKey::with_name_and_category(name, category),
                                    kind,
                                    cs_id,
                                    log_id,
                                )
                            })
                            .collect()
                        }
                    }
                    BookmarkPagination::After(after) => SelectByPrefixAfter::maybe_traced_query(
                        &conn,
                        cri,
                        &repo_id,
                        &prefix_like_pattern,
                        &"\\",
                        &after,
                        &limit,
                        &kinds,
                        &categories,
                    )
                    .await?
                    .into_iter()
                    .map(|(name, category, kind, cs_id, log_id)| {
                        (
                            BookmarkKey::with_name_and_category(name, category),
                            kind,
                            cs_id,
                            log_id,
                        )
                    })
                    .collect(),
                }
            };

            Ok(rows)
        }
    }

    pub fn get_raw(
        &self,
        ctx: CoreContext,
        key: &BookmarkKey,
    ) -> impl Future<Output = Result<Option<(ChangesetId, Option<u64>)>>> + 'static + use<> {
        STATS::get_bookmark.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        let conn = self.connections.read_master_connection.clone();
        cloned!(self.repo_id, key);
        async move {
            let rows = SelectBookmark::maybe_traced_query(
                &conn,
                ctx.client_request_info(),
                &repo_id,
                key.name(),
                key.category(),
            )
            .await?;
            Ok(rows.into_iter().next())
        }
    }
}

#[async_trait]
impl Bookmarks for SqlBookmarks {
    fn list(
        &self,
        ctx: CoreContext,
        freshness: Freshness,
        prefix: &BookmarkPrefix,
        categories: &[BookmarkCategory],
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        let fut = self.list_raw(
            &ctx, freshness, prefix, categories, kinds, pagination, limit,
        );

        async move {
            let rows = fut.await?;

            Ok(stream::iter(rows.into_iter().map(|row| {
                let (key, kind, changeset_id, _log_id) = row;
                Ok((Bookmark::new(key, kind), changeset_id))
            })))
        }
        .try_flatten_stream()
        .boxed()
    }

    fn get(
        &self,
        ctx: CoreContext,
        name: &BookmarkKey,
    ) -> BoxFuture<'static, Result<Option<ChangesetId>>> {
        self.get_raw(ctx, name)
            .map_ok(|maybe_row| maybe_row.map(|(cs_id, _log_id)| cs_id))
            .boxed()
    }

    async fn create_subscription(
        &self,
        ctx: &CoreContext,
        freshness: Freshness,
    ) -> Result<Box<dyn BookmarksSubscription>> {
        let sub = SqlBookmarksSubscription::create(ctx, self.clone(), freshness)
            .await
            .context("Failed to create SqlBookmarksSubscription")?;

        Ok(Box::new(sub))
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
        key: BookmarkKey,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>>
    {
        let conn = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            self.connections.read_master_connection.clone()
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            self.connections.read_connection.clone()
        };
        let repo_id = self.repo_id;

        async move {
            let tok: i32 = rand::thread_rng().r#gen();
            let cri = ctx.client_request_info();

            let rows = match offset {
                Some(offset) => {
                    SelectBookmarkLogsWithOffset::maybe_traced_query(
                        &conn,
                        cri,
                        &repo_id,
                        key.name(),
                        key.category(),
                        &max_rec,
                        &offset,
                        &tok,
                    )
                    .await?
                }
                None => {
                    SelectBookmarkLogs::maybe_traced_query(
                        &conn,
                        cri,
                        &repo_id,
                        key.name(),
                        key.category(),
                        &max_rec,
                        &tok,
                    )
                    .await?
                }
            };
            Ok(stream::iter(
                rows.into_iter()
                    .map(|(from_id, to_id, reason, ts, _)| (from_id, to_id, reason, ts))
                    .map(Ok),
            ))
        }
        .try_flatten_stream()
        .boxed()
    }

    fn list_bookmark_log_entries_ts_in_range(
        &self,
        ctx: CoreContext,
        key: BookmarkKey,
        max_rec: u32,
        min_ts: Timestamp,
        max_ts: Timestamp,
    ) -> BoxStream<'static, Result<(u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>>
    {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let conn = self.connections.read_connection.clone();
        let repo_id = self.repo_id;

        async move {
            let cri = ctx.client_request_info();
            let rows = SelectBookmarkLogsWithTsInRange::maybe_traced_query(
                &conn,
                cri,
                &repo_id,
                key.name(),
                key.category(),
                &max_rec,
                &min_ts,
                &max_ts,
            )
            .await?;
            Ok(stream::iter(rows.into_iter().map(Ok)))
        }
        .try_flatten_stream()
        .boxed()
    }

    fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: BookmarkUpdateLogId,
        maybe_exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<'static, Result<u64>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let conn = self.connections.read_connection.clone();
        let repo_id = self.repo_id;

        async move {
            let cri = ctx.client_request_info();
            let entries = match maybe_exclude_reason {
                Some(ref r) => {
                    CountFurtherBookmarkLogEntriesWithoutReason::maybe_traced_query(
                        &conn, cri, &id, &repo_id, r,
                    )
                    .await?
                }
                None => {
                    CountFurtherBookmarkLogEntries::maybe_traced_query(&conn, cri, &id, &repo_id)
                        .await?
                }
            };
            match entries.into_iter().next() {
                Some(count) => Ok(count.0),
                None => {
                    let extra = match maybe_exclude_reason {
                        Some(..) => "without reason",
                        None => "",
                    };
                    Err(anyhow!(
                        "Failed to query further bookmark log entries{}",
                        extra
                    ))
                }
            }
        }
        .boxed()
    }

    fn count_further_bookmark_log_entries_by_reason(
        &self,
        ctx: CoreContext,
        id: BookmarkUpdateLogId,
    ) -> BoxFuture<'static, Result<Vec<(BookmarkUpdateReason, u64)>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let conn = self.connections.read_connection.clone();
        let repo_id = self.repo_id;
        async move {
            let cri = ctx.client_request_info();
            let entries = CountFurtherBookmarkLogEntriesByReason::maybe_traced_query(
                &conn, cri, &id, &repo_id,
            )
            .await?;
            Ok(entries.into_iter().collect())
        }
        .boxed()
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: BookmarkUpdateLogId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<'static, Result<Option<u64>>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let conn = self.connections.read_connection.clone();
        cloned!(self.repo_id, reason);
        async move {
            let cri = ctx.client_request_info();
            let entries = SkipOverBookmarkLogEntriesWithReason::maybe_traced_query(
                &conn, cri, &id, &repo_id, &reason,
            )
            .await?;
            Ok(entries.first().map(|entry| entry.0))
        }
        .boxed()
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: BookmarkUpdateLogId,
        limit: u64,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let conn = self.connections.read_connection.clone();
        let repo_id = self.repo_id;

        async move {
            let cri = ctx.client_request_info();
            let entries =
                ReadNextBookmarkLogEntries::maybe_traced_query(&conn, cri, &id, &repo_id, &limit)
                    .watched(ctx.logger())
                    .await?;

            let homogenous_entries: Vec<_> = match entries.first().cloned() {
                Some(first_entry) => {
                    // Note: types are explicit here to protect us from query behavior change
                    //       when tuple items 2, 3, or 6 become something else, and we still succeed
                    //       compiling everything because of the type inference
                    let first_name: &BookmarkKey = &BookmarkKey::with_name_and_category(
                        first_entry.2.clone(),
                        first_entry.3.clone(),
                    );
                    let first_reason: &BookmarkUpdateReason = &first_entry.6;
                    entries
                        .into_iter()
                        .take_while(|entry| {
                            let name: &BookmarkKey = &BookmarkKey::with_name_and_category(
                                entry.2.clone(),
                                entry.3.clone(),
                            );
                            let reason: &BookmarkUpdateReason = &entry.6;
                            name == first_name && reason == first_reason
                        })
                        .collect()
                }
                None => entries.into_iter().collect(),
            };
            Ok(
                stream::iter(homogenous_entries.into_iter().map(Ok)).and_then(|entry| async move {
                    let (id, repo_id, name, category, to_cs_id, from_cs_id, reason, timestamp) =
                        entry;
                    Ok(BookmarkUpdateLogEntry {
                        id: BookmarkUpdateLogId(id.try_into()?),
                        repo_id,
                        bookmark_name: BookmarkKey::with_name_and_category(name, category),
                        to_changeset_id: to_cs_id,
                        from_changeset_id: from_cs_id,
                        reason,
                        timestamp,
                    })
                }),
            )
        }
        .try_flatten_stream()
        .boxed()
    }

    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: BookmarkUpdateLogId,
        limit: u64,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            self.connections.read_master_connection.clone()
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            self.connections.read_connection.clone()
        };

        let repo_id = self.repo_id;

        async move {
            let cri = ctx.client_request_info();
            let entries = ReadNextBookmarkLogEntries::maybe_traced_query(
                &connection,
                cri,
                &id,
                &repo_id,
                &limit,
            )
            .await?;

            Ok(
                stream::iter(entries.into_iter().map(Ok)).and_then(|entry| async move {
                    let (id, repo_id, name, category, to_cs_id, from_cs_id, reason, timestamp) =
                        entry;
                    Ok(BookmarkUpdateLogEntry {
                        id: id.try_into()?,
                        repo_id,
                        bookmark_name: BookmarkKey::with_name_and_category(name, category),
                        to_changeset_id: to_cs_id,
                        from_changeset_id: from_cs_id,
                        reason,
                        timestamp,
                    })
                }),
            )
        }
        .try_flatten_stream()
        .boxed()
    }

    fn get_largest_log_id(
        &self,
        ctx: CoreContext,
        freshness: Freshness,
    ) -> BoxFuture<'static, Result<Option<u64>>> {
        let connection = if freshness == Freshness::MostRecent {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            self.connections.read_master_connection.clone()
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            self.connections.read_connection.clone()
        };
        let repo_id = self.repo_id;

        async move {
            let cri = ctx.client_request_info();
            let entries = GetLargestLogId::maybe_traced_query(&connection, cri, &repo_id).await?;
            let entry = entries.into_iter().next();
            match entry {
                Some(count) => Ok(count.0),
                None => Err(anyhow!("Failed to query largest log id")),
            }
        }
        .boxed()
    }
}
