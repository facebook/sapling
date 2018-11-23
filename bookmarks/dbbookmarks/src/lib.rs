// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate bookmarks;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate mononoke_types;
#[macro_use]
extern crate sql;
extern crate sql_ext;
extern crate storage_types;

use std::collections::{HashMap, HashSet};

use bookmarks::{Bookmark, BookmarkPrefix, Bookmarks, Transaction};
use failure::{Error, Result};
use futures::{stream, Future, IntoFuture, future::{loop_fn, Loop}};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use sql::Connection;
pub use sql_ext::SqlConstructors;

use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;

#[derive(Clone)]
pub struct SqlBookmarks {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
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
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
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
        name: &Bookmark,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        SelectBookmark::query(&self.read_master_connection, &repo_id, &name)
            .map(|rows| rows.into_iter().next().map(|row| row.0))
            .boxify()
    }

    fn list_by_prefix(
        &self,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        self.list_by_prefix_impl(prefix, repo_id, &self.read_master_connection)
    }

    fn list_by_prefix_maybe_stale(
        &self,
        prefix: &BookmarkPrefix,
        repo_id: &RepositoryId,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        self.list_by_prefix_impl(prefix, repo_id, &self.read_connection)
    }

    fn create_transaction(&self, repoid: &RepositoryId) -> Box<Transaction> {
        Box::new(SqlBookmarksTransaction::new(
            self.write_connection.clone(),
            repoid.clone(),
        ))
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
