// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(duration_as_u128)]

extern crate ascii;
extern crate bookmarks;
#[macro_use]
extern crate cloned;
extern crate context;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate sql;
extern crate sql_ext;
#[macro_use]
extern crate stats;

use std::collections::{HashMap, HashSet};

use bookmarks::{Bookmark, BookmarkPrefix, Bookmarks, Transaction};
use context::CoreContext;
use failure::{Error, Fail, Result};
use futures::{stream, Future, IntoFuture, Stream, future::{loop_fn, Loop, Shared}};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use sql::Connection;
pub use sql_ext::SqlConstructors;

use mononoke_types::{ChangesetId, RepositoryId};

use stats::Timeseries;
use std::sync::{Arc, RwLock};
use std::time;

type BookmarkTuple = (Bookmark, ChangesetId);

#[derive(Clone)]
pub struct SqlBookmarks {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

#[derive(Fail, Debug, Clone)]
#[fail(display = "Query failed: {}", inner)]
pub struct ClonableError {
    inner: Arc<Error>,
}

impl From<Error> for ClonableError {
    fn from(e: Error) -> Self {
        Self { inner: Arc::new(e) }
    }
}

queries! {
    write ReplaceBookmarks(
        values: (repo_id: RepositoryId, name: Bookmark, changeset_id: ChangesetId)
    ) {
        none,
        "REPLACE INTO bookmarks (repo_id, name, changeset_id) VALUES {values}"
    }

    write InsertBookmarks(
        values: (repo_id: RepositoryId, name: Bookmark, changeset_id: ChangesetId)
    ) {
        none,
        "INSERT INTO bookmarks (repo_id, name, changeset_id) VALUES {values}"
    }

    write UpdateBookmark(
        repo_id: RepositoryId,
        name: Bookmark,
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

    write DeleteBookmark(repo_id: RepositoryId, name: Bookmark) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}"
    }

    write DeleteBookmarkIf(repo_id: RepositoryId, name: Bookmark, changeset_id: ChangesetId) {
        none,
        "DELETE FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
           AND changeset_id = {changeset_id}"
    }

    read SelectBookmark(repo_id: RepositoryId, name: Bookmark) -> (ChangesetId) {
        "SELECT changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}
           AND name = {name}
         LIMIT 1"
    }

    read SelectAll(repo_id: RepositoryId) -> (Bookmark, ChangesetId) {
        "SELECT name, changeset_id
         FROM bookmarks
         WHERE repo_id = {repo_id}"
    }

    read SelectByPrefix(repo_id: RepositoryId, prefix: BookmarkPrefix) -> (Bookmark, ChangesetId) {
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
        repo_id: &RepositoryId,
        conn: &Connection,
    ) -> BoxStream<BookmarkTuple, Error> {
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
        name: &Bookmark,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        SelectBookmark::query(&self.read_master_connection, &repo_id, &name)
            .map(|rows| rows.into_iter().next().map(|row| row.0))
            .boxify()
    }

    fn list_by_prefix(
        &self,
        _ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<BookmarkTuple, Error> {
        self.list_by_prefix_impl(prefix, repo_id, &self.read_master_connection)
    }

    fn list_by_prefix_maybe_stale(
        &self,
        _ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<BookmarkTuple, Error> {
        self.list_by_prefix_impl(prefix, repo_id, &self.read_connection)
    }

    fn create_transaction(&self, _ctx: CoreContext, repoid: &RepositoryId) -> Box<Transaction> {
        Box::new(SqlBookmarksTransaction::new(
            self.write_connection.clone(),
            repoid.clone(),
        ))
    }
}

struct BookmarkCache {
    updated: time::Instant,
    values: Option<Shared<BoxFuture<Vec<BookmarkTuple>, ClonableError>>>,
}

const MAX_CACHE_AGE_MILIS: u128 = 1000;

impl BookmarkCache {
    fn new() -> Self {
        Self {
            updated: time::Instant::now(),
            values: None,
        }
    }

    fn update_and_get_values<T: Bookmarks>(
        &mut self,
        ctx: CoreContext,
        inner: &T,
        repo_id: &RepositoryId,
    ) -> Shared<BoxFuture<Vec<BookmarkTuple>, ClonableError>> {
        self.updated = time::Instant::now();
        let shared_future = inner
            .list_by_prefix_maybe_stale(ctx.clone(), &BookmarkPrefix::empty(), repo_id)
            .collect()
            .map_err(|e| e.into())
            .boxify()
            .shared();
        self.values = Some(shared_future.clone());
        // NB: `update_and_get_values` is not equivalent to two separate
        // hypothetical actions: `update`, then `get_values` (think of `update`
        // as `update_and_get_values` but without the return value).
        // It is possible that the time to run the inner.list_by_prefix() query
        // takes longer than MAX_CACHE_AGE_MILIS. In this case, if we first did
        // a pure `update` and then `get_values`, we would've gotten `None`.
        // The `update_and_get_values` approach ensures that in such case, the newest
        // possible future is still returned, which is favorable IMO, although it breaks
        // the "no older than MAX_CACHE_AGE_MILIS" promise, strictly speaking.
        shared_future
    }

    fn usable(&self) -> bool {
        self.values.is_some() && self.updated.elapsed().as_millis() <= MAX_CACHE_AGE_MILIS
    }

    fn get_values(&self) -> Option<Shared<BoxFuture<Vec<BookmarkTuple>, ClonableError>>> {
        if self.usable() {
            self.values.clone()
        } else {
            None
        }
    }
}

pub struct CachedBookmarks<T: Bookmarks + Sized> {
    inner: T,
    cache: RwLock<HashMap<RepositoryId, Arc<RwLock<BookmarkCache>>>>,
}

define_stats! {
    prefix = "mononoke.bookmarks";
    cache_hit: timeseries(SUM),
    cache_miss: timeseries(SUM),
}

impl<T: Bookmarks> CachedBookmarks<T> {
    fn get_repo_cache(
        &self,
        ctx: CoreContext,
        repo_id: &RepositoryId,
    ) -> Arc<RwLock<BookmarkCache>> {
        let poisoned_lock_error = "poisoned bookmark cache lock";
        // optimistic: maybe repo is already in the cache and no write lock is needed
        // separate scope to drop the lock guard
        {
            let hashmap = self.cache.read().expect(poisoned_lock_error);
            if let Some(repo_cache_arc) = hashmap.get(repo_id) {
                // repo is in the cache, no need to grab a write lock
                return repo_cache_arc.clone();
            }
        }
        // by now we know that repo insertion is needed
        let mut hashmap = self.cache.write().expect(poisoned_lock_error);
        let repo_cache_arc = hashmap.entry(repo_id.clone()).or_insert_with(|| {
            info!(ctx.logger(), "CachedBookmarks: adding repo: {:?}", repo_id);
            Arc::new(RwLock::new(BookmarkCache::new()))
        });
        repo_cache_arc.clone()
    }

    fn get_cached_future(
        &self,
        ctx: CoreContext,
        repo_id: &RepositoryId,
    ) -> Shared<BoxFuture<Vec<BookmarkTuple>, ClonableError>> {
        let repo_cache = self.get_repo_cache(ctx.clone(), repo_id);
        // optimistic: maybe cache is usable without updating
        // separate scope to drop the lock guard
        let poisoned_lock_error = format!("poisoned bookmark cache lock for repo {:?}", repo_id);
        {
            let cache = repo_cache.read().expect(&poisoned_lock_error);
            if let Some(shared_future) = cache.get_values() {
                STATS::cache_hit.add_value(1);
                debug!(
                    ctx.logger(),
                    "CachedBookmarks: recent cache available for repo: {:?}",
                    repo_id
                );
                return shared_future.clone();
            }
        }
        // by now we know that the update is needed
        let mut cache = repo_cache.write().expect(&poisoned_lock_error);
        STATS::cache_miss.add_value(1);
        debug!(
            ctx.logger(),
            "CachedBookmarks: updating cache for repo: {:?}",
            repo_id
        );
        cache.update_and_get_values(ctx.clone(), &self.inner, repo_id)
    }
}

impl<T: Bookmarks> Bookmarks for CachedBookmarks<T> {
    fn get(
        &self,
        ctx: CoreContext,
        name: &Bookmark,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        self.inner.get(ctx, name, repo_id)
    }

    fn list_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<BookmarkTuple, Error> {
        self.inner.list_by_prefix(ctx, prefix, repo_id)
    }

    fn list_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<BookmarkTuple, Error> {
        cloned!(prefix);
        self.get_cached_future(ctx.clone(), repo_id)
            .map(|v| stream::iter_ok((*v).clone()))
            .map_err(|e| (*e).clone().into())
            .flatten_stream()
            .filter(move |bc| prefix.is_prefix_of(&bc.0))
            .boxify()
    }

    fn create_transaction(&self, ctx: CoreContext, repoid: &RepositoryId) -> Box<Transaction> {
        self.inner.create_transaction(ctx, repoid)
    }
}

impl SqlConstructors for CachedBookmarks<SqlBookmarks> {
    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            inner: SqlBookmarks {
                write_connection,
                read_connection,
                read_master_connection,
            },
            cache: RwLock::new(HashMap::new()),
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-bookmarks.sql")
    }
}

struct SqlBookmarksTransaction {
    write_connection: Connection,
    repo_id: RepositoryId,
    force_sets: HashMap<Bookmark, ChangesetId>,
    creates: HashMap<Bookmark, ChangesetId>,
    sets: HashMap<Bookmark, BookmarkSetData>,
    force_deletes: HashSet<Bookmark>,
    deletes: HashMap<Bookmark, ChangesetId>,
}

impl SqlBookmarksTransaction {
    fn new(write_connection: Connection, repo_id: RepositoryId) -> Self {
        Self {
            write_connection,
            repo_id,
            force_sets: HashMap::new(),
            creates: HashMap::new(),
            sets: HashMap::new(),
            force_deletes: HashSet::new(),
            deletes: HashMap::new(),
        }
    }

    fn check_if_bookmark_already_used(&self, key: &Bookmark) -> Result<()> {
        if self.creates.contains_key(key) || self.force_sets.contains_key(key)
            || self.sets.contains_key(key) || self.force_deletes.contains(key)
            || self.deletes.contains_key(key)
        {
            bail_msg!("{} bookmark was already used", key);
        }
        Ok(())
    }
}

impl Transaction for SqlBookmarksTransaction {
    fn update(&mut self, key: &Bookmark, new_cs: &ChangesetId, old_cs: &ChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.sets.insert(
            key.clone(),
            BookmarkSetData {
                new_cs: *new_cs,
                old_cs: *old_cs,
            },
        );
        Ok(())
    }

    fn create(&mut self, key: &Bookmark, new_cs: &ChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.creates.insert(key.clone(), *new_cs);
        Ok(())
    }

    fn force_set(&mut self, key: &Bookmark, new_cs: &ChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_sets.insert(key.clone(), *new_cs);
        Ok(())
    }

    fn delete(&mut self, key: &Bookmark, old_cs: &ChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.deletes.insert(key.clone(), *old_cs);
        Ok(())
    }

    fn force_delete(&mut self, key: &Bookmark) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_deletes.insert(key.clone());
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

        write_connection
            .start_transaction()
            .map_err(Some)
            .and_then(move |transaction| {
                let force_set: Vec<_> = force_sets.into_iter().collect();
                let mut ref_rows = Vec::new();
                for idx in 0..force_set.len() {
                    ref_rows.push((&repo_id, &force_set[idx].0, &force_set[idx].1))
                }
                ReplaceBookmarks::query_with_transaction(transaction, &ref_rows[..]).map_err(Some)
            })
            .and_then(move |(transaction, _)| {
                let creates: Vec<_> = creates.into_iter().collect();
                let mut ref_rows = Vec::new();
                for idx in 0..creates.len() {
                    ref_rows.push((&repo_id, &creates[idx].0, &creates[idx].1))
                }
                InsertBookmarks::query_with_transaction(transaction, &ref_rows[..]).map_err(Some)
            })
            .and_then(move |(transaction, _)| {
                loop_fn(
                    (transaction, sets.into_iter()),
                    move |(transaction, mut updates)| match updates.next() {
                        Some((name, BookmarkSetData { new_cs, old_cs })) => {
                            UpdateBookmark::query_with_transaction(
                                transaction,
                                &repo_id,
                                &name,
                                &old_cs,
                                &new_cs,
                            ).then(move |res| match res {
                                Err(err) => Err(Some(err)),
                                Ok((transaction, result)) => if result.affected_rows() == 1 {
                                    Ok((transaction, updates))
                                } else {
                                    Err(None)
                                },
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
                    (transaction, force_deletes.into_iter()),
                    move |(transaction, mut deletes)| match deletes.next() {
                        Some(name) => {
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
                        Some((name, old_cs)) => DeleteBookmarkIf::query_with_transaction(
                            transaction,
                            &repo_id,
                            &name,
                            &old_cs,
                        ).then(move |res| match res {
                            Err(err) => Err(Some(err)),
                            Ok((transaction, result)) => if result.affected_rows() == 1 {
                                Ok((transaction, deletes))
                            } else {
                                Err(None)
                            },
                        })
                            .map(Loop::Continue)
                            .left_future(),
                        None => Ok(Loop::Break(transaction)).into_future().right_future(),
                    },
                )
            })
            .then(|result| match result {
                Ok(transaction) => transaction.commit().and_then(|()| Ok(true)).left_future(),
                Err(None) => Ok(false).into_future().right_future(),
                Err(Some(err)) => Err(err).into_future().right_future(),
            })
            .boxify()
    }
}

struct BookmarkSetData {
    new_cs: ChangesetId,
    old_cs: ChangesetId,
}
