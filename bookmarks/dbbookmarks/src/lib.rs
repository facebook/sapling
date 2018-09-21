// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]
// FIXME T34253207, remove when https://github.com/diesel-rs/diesel/issues/1785 fixed
#![allow(proc_macro_derive_resolution_fallback)]

extern crate ascii;
extern crate bookmarks;
extern crate db;
extern crate db_conn;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate mononoke_types;
extern crate storage_types;

mod schema;
mod models;

use bookmarks::{Bookmark, BookmarkPrefix, Bookmarks, Transaction};
use db_conn::{MysqlConnInner, SqliteConnInner};
use diesel::{delete, insert_into, replace_into, update, MysqlConnection, SqliteConnection};
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use failure::{Error, Result};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use db::ConnectionParams;
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;
use std::collections::{HashMap, HashSet};
use std::result;
use std::sync::MutexGuard;

#[derive(Clone)]
pub struct SqliteDbBookmarks {
    inner: SqliteConnInner,
}

impl SqliteDbBookmarks {
    fn from(inner: SqliteConnInner) -> SqliteDbBookmarks {
        SqliteDbBookmarks { inner } // one true constructor
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-bookmarks.sql")
    }

    pub fn in_memory() -> Result<Self> {
        Ok(Self::from(SqliteConnInner::in_memory(
            Self::get_up_query(),
        )?))
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P) -> Result<Self> {
        Ok(Self::from(SqliteConnInner::open_or_create(
            path,
            Self::get_up_query(),
        )?))
    }

    pub fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        self.inner.get_master_conn()
    }
}

#[derive(Clone)]
pub struct MysqlDbBookmarks {
    inner: MysqlConnInner,
}

impl MysqlDbBookmarks {
    fn from(inner: MysqlConnInner) -> MysqlDbBookmarks {
        MysqlDbBookmarks { inner } // one true constructor
    }

    pub fn open(params: &ConnectionParams) -> Result<Self> {
        Ok(Self::from(MysqlConnInner::open_with_params(
            params,
            params,
        )?))
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/mysql-bookmarks.sql")
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        Ok(Self::from(MysqlConnInner::create_test_db(
            prefix,
            Self::get_up_query(),
        )?))
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_master_conn()
    }
}

macro_rules! impl_bookmarks {
    ($struct: ty, $transaction_struct: ident) => {
        impl Bookmarks for $struct {
            fn get(
                &self,
                name: &Bookmark,
                repo_id: &RepositoryId,
            ) -> BoxFuture<Option<ChangesetId>, Error> {
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = try_boxfuture!(self.get_conn());

                schema::bookmarks::table
                    .filter(schema::bookmarks::repo_id.eq(repo_id))
                    .filter(schema::bookmarks::name.eq(name.to_string()))
                    .select(schema::bookmarks::changeset_id)
                    .first::<ChangesetId>(&*connection)
                    .optional()
                    .into_future()
                    .from_err()
                    .boxify()
            }

            fn list_by_prefix(
                &self,
                prefix: &BookmarkPrefix,
                repo_id: &RepositoryId,
            ) -> BoxStream<(Bookmark, ChangesetId), Error> {
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = match self.get_conn() {
                    Ok(conn) => conn,
                    Err(err) => {
                        return stream::once(Err(err)).boxify();
                    },
                };

                let query = schema::bookmarks::table
                    .filter(schema::bookmarks::repo_id.eq(repo_id))
                    .filter(schema::bookmarks::name.like(format!("{}%", prefix.to_string())));

                query
                    .get_results::<models::BookmarkRow>(&*connection)
                    .into_future()
                    .and_then(|bookmarks| {
                        let bookmarks = bookmarks
                            .into_iter()
                            .map(|row| (row.name, row.changeset_id));
                        Ok(stream::iter_ok(bookmarks).boxify())
                    })
                    .from_err()
                    .flatten_stream()
                    .and_then(|entry| Ok((Bookmark::new(entry.0)?, entry.1)))
                    .boxify()
            }

            fn create_transaction(&self, repoid: &RepositoryId) -> Box<Transaction> {
                Box::new($transaction_struct::new(
                    self.clone(),
                    repoid,
                ))
            }
        }

        struct $transaction_struct {
            db: $struct,
            force_sets: HashMap<Bookmark, ChangesetId>,
            creates: HashMap<Bookmark, ChangesetId>,
            sets: HashMap<Bookmark, BookmarkSetData>,
            force_deletes: HashSet<Bookmark>,
            deletes: HashMap<Bookmark, ChangesetId>,
            repo_id: RepositoryId,
        }

        impl $transaction_struct {
            fn new(db: $struct, repo_id: &RepositoryId) -> Self {
                Self {
                    db,
                    force_sets: HashMap::new(),
                    creates: HashMap::new(),
                    sets: HashMap::new(),
                    force_deletes: HashSet::new(),
                    deletes: HashMap::new(),
                    repo_id: *repo_id,
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

        impl Transaction for $transaction_struct {
            fn update(
                &mut self,
                key: &Bookmark,
                new_cs: &ChangesetId,
                old_cs: &ChangesetId,
            ) -> Result<()> {
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

            fn commit(&self) -> BoxFuture<bool, Error> {
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = try_boxfuture!(self.db.get_conn());

                let txnres = connection.transaction::<_, Error, _>(|| {
                    replace_into(schema::bookmarks::table)
                        .values(&create_bookmarks_rows(self.repo_id, &self.force_sets))
                        .execute(&*connection)?;

                    insert_into(schema::bookmarks::table)
                        .values(&create_bookmarks_rows(self.repo_id, &self.creates))
                        .execute(&*connection)?;

                    for (key, &BookmarkSetData { new_cs, old_cs }) in self.sets.iter() {
                        let key = key.to_string();
                        let num_affected_rows = update(
                            schema::bookmarks::table
                                .filter(schema::bookmarks::repo_id.eq(self.repo_id))
                                .filter(schema::bookmarks::name.eq(key.clone()))
                                .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                        ).set(schema::bookmarks::changeset_id.eq(new_cs))
                            .execute(&*connection)?;
                        if num_affected_rows != 1 {
                            return Ok(false) // conflict
                        }
                    }

                    for key in self.force_deletes.iter() {
                        let key = key.to_string();
                        delete(schema::bookmarks::table
                                .filter(schema::bookmarks::repo_id.eq(self.repo_id))
                                .filter(schema::bookmarks::name.eq(key))
                            )
                            .execute(&*connection)?;
                    }

                    for (key, old_cs) in self.deletes.iter() {
                        let key = key.to_string();
                        let num_deleted_rows = delete(
                            schema::bookmarks::table
                                .filter(schema::bookmarks::repo_id.eq(self.repo_id))
                                .filter(schema::bookmarks::name.eq(key.clone()))
                                .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                        ).execute(&*connection)?;
                        if num_deleted_rows != 1 {
                            return Ok(false) // conflict
                        }
                    }
                    Ok(true)
                });
                future::result(txnres).from_err().boxify()
            }
        }
    }
}

impl_bookmarks!(SqliteDbBookmarks, SqliteBookmarksTransaction);
impl_bookmarks!(MysqlDbBookmarks, MysqlBookmarksTransaction);

struct BookmarkSetData {
    new_cs: ChangesetId,
    old_cs: ChangesetId,
}

fn create_bookmarks_rows(
    repo_id: RepositoryId,
    map: &HashMap<Bookmark, ChangesetId>,
) -> Vec<models::BookmarkRow> {
    map.iter()
        .map(|(name, changeset_id)| models::BookmarkRow {
            repo_id,
            name: name.to_string(),
            changeset_id: *changeset_id,
        })
        .collect()
}
