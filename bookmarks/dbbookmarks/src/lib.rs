// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use bookmarks::{
    BookmarkName, BookmarkPrefix, BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks,
    BundleReplayData, Transaction,
};
use context::CoreContext;
use failure_ext::{bail_msg, err_msg, Error, Result};
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

define_stats! {
    prefix = "mononoke.dbbookmarks";
    list_by_prefix_maybe_stale: timeseries(RATE, SUM),
    list_by_prefix: timeseries(RATE, SUM),
    get_bookmark: timeseries(RATE, SUM),
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
        values: (repo_id: RepositoryId, name: BookmarkName, changeset_id: ChangesetId)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bookmarks (repo_id, name, changeset_id) VALUES {values}"
    }

    write UpdateBookmark(
        repo_id: RepositoryId,
        name: BookmarkName,
        old_id: ChangesetId,
        new_id: ChangesetId,
    ) {
        none,
        "UPDATE bookmarks
         SET changeset_id = {new_id}
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND changeset_id = {old_id}"
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
         (repo_id, name, from_changeset_id, to_changeset_id, reason, timestamp)
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

    read SelectAll(repo_id: RepositoryId) -> (BookmarkName, ChangesetId) {
        "SELECT name, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}"
    }

    read SelectByPrefix(repo_id: RepositoryId, prefix: BookmarkPrefix) -> (BookmarkName, ChangesetId) {
        mysql(
            "SELECT name, changeset_id
             FROM bookmarks
             WHERE repo_id = {repo_id}
               AND name LIKE CONCAT({prefix}, '%')"
        )
        sqlite(
            "SELECT name, changeset_id
             FROM bookmarks
             WHERE repo_id = {repo_id}
               AND name LIKE {prefix} || '%'"
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

impl SqlBookmarks {
    fn list_by_prefix_impl(
        &self,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        conn: &Connection,
    ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
        if prefix.is_empty() {
            SelectAll::query(&conn, &repo_id)
                .map(|rows| stream::iter_ok(rows))
                .flatten_stream()
                .boxify()
        } else {
            SelectByPrefix::query(&conn, &repo_id, &prefix)
                .map(|rows| stream::iter_ok(rows))
                .flatten_stream()
                .boxify()
        }
    }
}

impl Bookmarks for SqlBookmarks {
    fn get(
        &self,
        _ctx: CoreContext,
        name: &BookmarkName,
        repo_id: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        SelectBookmark::query(&self.read_master_connection, &repo_id, &name)
            .map(|rows| rows.into_iter().next().map(|row| row.0))
            .boxify()
    }

    fn list_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        name: BookmarkName,
        repo_id: RepositoryId,
        max_rec: u32,
    ) -> BoxStream<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error> {
        SelectBookmarkLogs::query(&self.read_master_connection, &repo_id, &name, &max_rec)
            .map(|rows| stream::iter_ok(rows))
            .flatten_stream()
            .boxify()
    }

    fn list_by_prefix(
        &self,
        _ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
    ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
        STATS::list_by_prefix.add_value(1);
        self.list_by_prefix_impl(prefix, repo_id, &self.read_master_connection)
    }

    fn list_by_prefix_maybe_stale(
        &self,
        _ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
    ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
        STATS::list_by_prefix_maybe_stale.add_value(1);
        self.list_by_prefix_impl(prefix, repo_id, &self.read_connection)
    }

    fn create_transaction(&self, _ctx: CoreContext, repoid: RepositoryId) -> Box<Transaction> {
        Box::new(SqlBookmarksTransaction::new(
            self.write_connection.clone(),
            repoid.clone(),
        ))
    }

    fn count_further_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        maybe_exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<u64, Error> {
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
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<Vec<(BookmarkUpdateReason, u64)>, Error> {
        CountFurtherBookmarkLogEntriesByReason::query(&self.read_connection, &id, &repoid)
            .map(|entries| entries.into_iter().collect())
            .boxify()
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<Option<u64>, Error> {
        SkipOverBookmarkLogEntriesWithReason::query(&self.read_connection, &id, &repoid, &reason)
            .map(|entries| entries.first().map(|entry| entry.0))
            .boxify()
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
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
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
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

struct SqlBookmarksTransaction {
    write_connection: Connection,
    repo_id: RepositoryId,
    force_sets: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    creates: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
    sets: HashMap<BookmarkName, (BookmarkSetData, BookmarkUpdateReason)>,
    force_deletes: HashMap<BookmarkName, BookmarkUpdateReason>,
    deletes: HashMap<BookmarkName, (ChangesetId, BookmarkUpdateReason)>,
}

impl SqlBookmarksTransaction {
    fn new(write_connection: Connection, repo_id: RepositoryId) -> Self {
        Self {
            write_connection,
            repo_id,
            force_sets: HashMap::new(),
            creates: HashMap::new(),
            sets: HashMap::new(),
            force_deletes: HashMap::new(),
            deletes: HashMap::new(),
        }
    }

    fn check_if_bookmark_already_used(&self, key: &BookmarkName) -> Result<()> {
        if self.creates.contains_key(key)
            || self.force_sets.contains_key(key)
            || self.sets.contains_key(key)
            || self.force_deletes.contains_key(key)
            || self.deletes.contains_key(key)
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
        loop_fn(
            (moves.into_iter(), sql_transaction),
            move |(mut moves, sql_transaction)| match moves.next() {
                Some((bookmark, (from_changeset_id, to_changeset_id, reason))) => {
                    let row = vec![(
                        &repo_id,
                        &bookmark,
                        &from_changeset_id,
                        &to_changeset_id,
                        &reason,
                        &timestamp,
                    )];
                    let reason = reason.clone();
                    AddBookmarkLog::query_with_transaction(sql_transaction, &row[..])
                        .and_then(move |(sql_transaction, query_result)| {
                            if let Some(id) = query_result.last_insert_id() {
                                Self::log_bundle_replay_data(id, reason, sql_transaction)
                                    .map(move |sql_transaction| (moves, sql_transaction))
                                    .left_future()
                            } else {
                                future::err(err_msg("failed to insert bookmark log entry"))
                                    .right_future()
                            }
                        })
                        .map(Loop::Continue)
                        .left_future()
                }
                None => future::ok(Loop::Break(sql_transaction)).right_future(),
            },
        )
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
        } = this;

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

        write_connection
            .start_transaction()
            .map_err(Some)
            .and_then(move |transaction| {
                let force_set: Vec<_> = force_sets.clone().into_iter().collect();
                let mut ref_rows = Vec::new();
                for idx in 0..force_set.len() {
                    let (ref to_changeset_id, _) = force_set[idx].1;
                    ref_rows.push((&repo_id, &force_set[idx].0, to_changeset_id));
                }

                ReplaceBookmarks::query_with_transaction(transaction, &ref_rows[..]).map_err(Some)
            })
            .and_then(move |(transaction, _)| {
                let creates_vec: Vec<_> = creates.clone().into_iter().collect();
                let mut ref_rows = Vec::new();
                for idx in 0..creates_vec.len() {
                    let (ref to_changeset_id, _) = creates_vec[idx].1;
                    ref_rows.push((&repo_id, &creates_vec[idx].0, to_changeset_id))
                }

                let rows_to_insert = creates_vec.len() as u64;
                InsertBookmarks::query_with_transaction(transaction, &ref_rows[..]).then(
                    move |res| match res {
                        Err(err) => Err(Some(err)),
                        Ok((transaction, result)) => {
                            if result.affected_rows() == rows_to_insert {
                                Ok(transaction)
                            } else {
                                Err(None)
                            }
                        }
                    },
                )
            })
            .and_then(move |transaction| {
                loop_fn(
                    (transaction, sets.into_iter()),
                    move |(transaction, mut updates)| match updates.next() {
                        Some((
                            ref name,
                            (
                                BookmarkSetData {
                                    ref new_cs,
                                    ref old_cs,
                                },
                                ref _reason,
                            ),
                        )) if new_cs == old_cs => {
                            // no-op update. If bookmark points to a correct update then
                            // let's continue the transaction, otherwise revert it
                            SelectBookmark::query_with_transaction(transaction, &repo_id, &name)
                                .then({
                                    let new_cs = new_cs.clone();
                                    move |res| match res {
                                        Err(err) => Err(Some(err)),
                                        Ok((transaction, result)) => {
                                            if result.get(0).map(|b| b.0) == Some(new_cs) {
                                                Ok((transaction, updates))
                                            } else {
                                                Err(None)
                                            }
                                        }
                                    }
                                })
                                .map(Loop::Continue)
                                .boxify()
                        }
                        Some((name, (BookmarkSetData { new_cs, old_cs }, _reason))) => {
                            UpdateBookmark::query_with_transaction(
                                transaction,
                                &repo_id,
                                &name,
                                &old_cs,
                                &new_cs,
                            )
                            .then(move |res| match res {
                                Err(err) => Err(Some(err)),
                                Ok((transaction, result)) => {
                                    if result.affected_rows() == 1 {
                                        Ok((transaction, updates))
                                    } else {
                                        Err(None)
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
                                    Err(err) => Err(Some(err)),
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
                                Err(err) => Err(Some(err)),
                                Ok((transaction, result)) => {
                                    if result.affected_rows() == 1 {
                                        Ok((transaction, deletes))
                                    } else {
                                        Err(None)
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
                    .map_err(Some)
            })
            .then(|result| match result {
                Ok(transaction) => transaction.commit().and_then(|()| Ok(true)).left_future(),
                Err(None) => Ok(false).into_future().right_future(),
                Err(Some(err)) => Err(err).into_future().right_future(),
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
