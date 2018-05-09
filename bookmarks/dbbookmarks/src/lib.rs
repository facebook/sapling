// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate ascii;
extern crate bookmarks;
extern crate db;
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
extern crate storage_types;

mod schema;
mod models;

use bookmarks::{Bookmark, BookmarkPrefix, Bookmarks, Transaction};
use diesel::{delete, insert_into, replace_into, update, MysqlConnection, SqliteConnection};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use failure::{Error, Result};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use db::ConnectionParams;
use mercurial_types::{DChangesetId, RepositoryId};
use std::collections::{HashMap, HashSet};
use std::result;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone)]
pub struct SqliteDbBookmarks {
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteDbBookmarks {
    /// Open a SQLite database. This is synchronous because the SQLite backend hits local
    /// disk or memory.
    pub fn open<P: AsRef<str>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let conn = SqliteConnection::establish(path)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn)),
        })
    }

    fn create_tables(&mut self) -> Result<()> {
        let up_query = include_str!("../schemas/sqlite-bookmarks.sql");

        self.connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(())
    }

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P) -> Result<Self> {
        let mut bookmarks = Self::open(path)?;

        bookmarks.create_tables()?;

        Ok(bookmarks)
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P) -> Result<Self> {
        let mut bookmarks = Self::open(path)?;

        let _ = bookmarks.create_tables();

        Ok(bookmarks)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Self::create(":memory:")
    }

    fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }
}

#[derive(Clone)]
pub struct MysqlDbBookmarks {
    pool: Pool<ConnectionManager<MysqlConnection>>,
}

impl MysqlDbBookmarks {
    pub fn open(params: ConnectionParams) -> Result<Self> {
        let url = params.to_diesel_url()?;
        let manager = ConnectionManager::new(url);
        let pool = Pool::builder()
            .max_size(10)
            .min_idle(Some(1))
            .build(manager)?;
        Ok(Self { pool })
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        let params = db::create_test_db(prefix)?;
        Self::create(params)
    }

    fn create(params: ConnectionParams) -> Result<Self> {
        let changesets = Self::open(params)?;

        let up_query = include_str!("../schemas/mysql-bookmarks.sql");
        changesets.pool.get()?.batch_execute(&up_query)?;

        Ok(changesets)
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.pool.get().map_err(Error::from)
    }
}

macro_rules! impl_bookmarks {
    ($struct: ty, $transaction_struct: ident) => {
        impl Bookmarks for $struct {
            fn get(
                &self,
                name: &Bookmark,
                repo_id: &RepositoryId,
            ) -> BoxFuture<Option<DChangesetId>, Error> {
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = try_boxfuture!(self.get_conn());

                schema::bookmarks::table
                    .filter(schema::bookmarks::repo_id.eq(repo_id))
                    .filter(schema::bookmarks::name.eq(name.to_string()))
                    .select(schema::bookmarks::changeset_id)
                    .first::<DChangesetId>(&*connection)
                    .optional()
                    .into_future()
                    .from_err()
                    .boxify()
            }

            fn list_by_prefix(
                &self,
                prefix: &BookmarkPrefix,
                repo_id: &RepositoryId,
            ) -> BoxStream<(Bookmark, DChangesetId), Error> {
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
            force_sets: HashMap<Bookmark, DChangesetId>,
            creates: HashMap<Bookmark, DChangesetId>,
            sets: HashMap<Bookmark, BookmarkSetData>,
            force_deletes: HashSet<Bookmark>,
            deletes: HashMap<Bookmark, DChangesetId>,
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
                new_cs: &DChangesetId,
                old_cs: &DChangesetId,
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

            fn create(&mut self, key: &Bookmark, new_cs: &DChangesetId) -> Result<()> {
                self.check_if_bookmark_already_used(key)?;
                self.creates.insert(key.clone(), *new_cs);
                Ok(())
            }

            fn force_set(&mut self, key: &Bookmark, new_cs: &DChangesetId) -> Result<()> {
                self.check_if_bookmark_already_used(key)?;
                self.force_sets.insert(key.clone(), *new_cs);
                Ok(())
            }

            fn delete(&mut self, key: &Bookmark, old_cs: &DChangesetId) -> Result<()> {
                self.check_if_bookmark_already_used(key)?;
                self.deletes.insert(key.clone(), *old_cs);
                Ok(())
            }

            fn force_delete(&mut self, key: &Bookmark) -> Result<()> {
                self.check_if_bookmark_already_used(key)?;
                self.force_deletes.insert(key.clone());
                Ok(())
            }

            fn commit(&self) -> BoxFuture<(), Error> {
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
                                .filter(schema::bookmarks::name.eq(key.clone()))
                                .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                        ).set(schema::bookmarks::changeset_id.eq(new_cs))
                            .execute(&*connection)?;
                        if num_affected_rows != 1 {
                            bail_msg!("cannot update bookmark {}", key);
                        }
                    }

                    for key in self.force_deletes.iter() {
                        let key = key.to_string();
                        delete(schema::bookmarks::table.filter(schema::bookmarks::name.eq(key)))
                            .execute(&*connection)?;
                    }

                    for (key, old_cs) in self.deletes.iter() {
                        let key = key.to_string();
                        let num_deleted_rows = delete(
                            schema::bookmarks::table
                                .filter(schema::bookmarks::name.eq(key.clone()))
                                .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                        ).execute(&*connection)?;
                        if num_deleted_rows != 1 {
                            bail_msg!("cannot delete bookmark {}", key);
                        }
                    }
                    Ok(())
                });
                future::result(txnres).from_err().boxify()
            }
        }
    }
}

impl_bookmarks!(SqliteDbBookmarks, SqliteBookmarksTransaction);
impl_bookmarks!(MysqlDbBookmarks, MysqlBookmarksTransaction);

struct BookmarkSetData {
    new_cs: DChangesetId,
    old_cs: DChangesetId,
}

fn create_bookmarks_rows(
    repo_id: RepositoryId,
    map: &HashMap<Bookmark, DChangesetId>,
) -> Vec<models::BookmarkRow> {
    map.iter()
        .map(|(name, changeset_id)| models::BookmarkRow {
            repo_id,
            name: name.to_string(),
            changeset_id: *changeset_id,
        })
        .collect()
}
