// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from, never_type)]

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;

extern crate db;
extern crate futures_ext;
extern crate mercurial_types;

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::result;
use std::sync::{Mutex, MutexGuard};

use diesel::{insert_into, Connection, MysqlConnection, SqliteConnection};
use diesel::backend::Backend;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_types::HasSqlType;
use failure::ResultExt;
use futures::future;

use db::ConnectionParams;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, RepositoryId};
use mercurial_types::sql_types::HgChangesetIdSql;

mod errors;
mod schema;
mod models;
mod wrappers;

pub use errors::*;
use models::{ChangesetInsertRow, ChangesetParentRow, ChangesetRow};
use schema::{changesets, csparents};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangesetEntry {
    pub repo_id: RepositoryId,
    pub cs_id: HgChangesetId,
    pub parents: Vec<HgChangesetId>,
    pub gen: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangesetInsert {
    pub repo_id: RepositoryId,
    pub cs_id: HgChangesetId,
    pub parents: Vec<HgChangesetId>,
}

/// Interface to storage of changesets that have been completely stored in Mononoke.
pub trait Changesets: Send + Sync {
    /// Add a new entry to the changesets table.
    fn add(&self, cs: ChangesetInsert) -> BoxFuture<(), Error>;

    /// Retrieve the row specified by this commit, if available.
    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error>;
}

pub struct SqliteChangesets {
    connection: Mutex<SqliteConnection>,
}

impl SqliteChangesets {
    /// Open a SQLite database. This is synchronous because the SQLite backend hits local
    /// disk or memory.
    pub fn open<P: AsRef<str>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let conn = SqliteConnection::establish(path)?;
        Ok(Self {
            connection: Mutex::new(conn),
        })
    }

    fn create_tables(&mut self) -> Result<()> {
        let up_query = include_str!("../schemas/sqlite-changesets.sql");

        self.connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(())
    }

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P) -> Result<Self> {
        let mut changesets = Self::open(path)?;

        changesets.create_tables()?;

        Ok(changesets)
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P) -> Result<Self> {
        let mut changesets = Self::open(path)?;

        let _ = changesets.create_tables();

        Ok(changesets)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Self::create(":memory:")
    }

    pub fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }
}

pub struct MysqlChangesets {
    pool: Pool<ConnectionManager<MysqlConnection>>,
}

impl MysqlChangesets {
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

        let up_query = include_str!("../schemas/mysql-changesets.sql");
        changesets.pool.get()?.batch_execute(&up_query)?;

        Ok(changesets)
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.pool.get().map_err(Error::from)
    }
}

/// Using a macro here is unfortunate, but it appears to be the only way to share this code
/// between SQLite and MySQL.
/// See https://github.com/diesel-rs/diesel/issues/882#issuecomment-300257476
macro_rules! impl_changesets {
    ($struct: ty) => {
        impl Changesets for $struct {
            /// Retrieve the changeset specified by this commit.
            fn get(
                &self,
                repo_id: RepositoryId,
                cs_id: HgChangesetId,
            ) -> BoxFuture<Option<ChangesetEntry>, Error> {
                // TODO: don't block -- send this to another thread
                let query = changeset_query(repo_id, cs_id);
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = match self.get_conn() {
                    Ok(conn) => conn,
                    Err(err) => return future::err(err).boxify(),
                };
                // TODO: (sid0) T26215418 run the changeset and parent queries in parallel, once
                // async framework is available
                let changeset_row = query.first::<ChangesetRow>(&*connection).optional();
                // This code is written in this style to allow easy porting to futures.
                let entry = changeset_row.map_err(failure::Error::from).and_then(|row| {
                    match row {
                        None => Ok(None),
                        Some(row) => {
                            // Diesel can't express unsigned ints, so convert manually.
                            // TODO: (sid0) T26215455 hide i64/u64 Diesel conversions behind an
                            // interface
                            let gen = u64::try_from(row.gen)
                                .context(ErrorKind::InvalidStoredData)?;

                            let parent_query = csparents::table
                                .filter(csparents::cs_id.eq(row.id))
                                .order(csparents::seq.asc())
                                .inner_join(changesets::table);
                            let parent_rows = parent_query
                                .load::<(ChangesetParentRow, ChangesetRow)>(&*connection);

                            parent_rows.map(|parents| {
                                Some(ChangesetEntry {
                                    repo_id: row.repo_id,
                                    cs_id: row.cs_id,
                                    parents: parents.into_iter().map(|p| p.1.cs_id).collect(),
                                    gen,
                                })
                            }).map_err(failure::Error::from)
                        }
                    }
                });
                future::result(entry).boxify()
            }

            /// Insert a new changeset into this table. Checks that all parents are already in
            /// storage.
            fn add(&self, cs: ChangesetInsert) -> BoxFuture<(), Error> {
                let parent_query = changesets::table
                    .filter(changesets::repo_id.eq(cs.repo_id))
                    .filter(changesets::cs_id.eq_any(&cs.parents));
                #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
                let connection = match self.get_conn() {
                    Ok(conn) => conn,
                    Err(err) => return future::err(err).boxify(),
                };

                // TODO: always hit master for this query?
                let parent_rows = parent_query.load::<ChangesetRow>(&*connection);
                // This code is written in this style to allow easy porting to futures.
                let txn_result = parent_rows.map_err(failure::Error::from).and_then(|parent_rows| {
                    check_missing_rows(&cs.parents, &parent_rows)?;

                    // A changeset with no parents has generation number 1.
                    // (The null commit has generation number 0.)
                    let gen = parent_rows.iter().map(|row| row.gen).max().unwrap_or(0) + 1;
                    let cs_insert = ChangesetInsertRow {
                        repo_id: cs.repo_id,
                        cs_id: cs.cs_id,
                        gen,
                    };

                    connection.transaction::<_, Error, _>(|| {
                        // TODO figure out how to make transactions async. Assuming for now that
                        // the inside of a transaction can be synchronous.
                        let result = insert_into(changesets::table)
                            .values(&cs_insert)
                            .execute(&*connection);
                        map_add_result(result)?;

                        let cs_query = changeset_query(cs.repo_id, cs.cs_id);
                        // MySQL and SQLite both have functions to expose "last insert ID", but
                        // Diesel doesn't expose them. Using it isn't strictly necessary, because
                        // inserts can be later queried from selects within the same transaction.
                        // But doing so would probably save a roundtrip.
                        // TODO: (sid0) T26215642 expose last_insert_id in Diesel and use it.
                        let new_cs_row = cs_query.first::<ChangesetRow>(&*connection)?;

                        // parent_rows might not be in the same order as cs.parents.
                        let parent_map: HashMap<_, _> = parent_rows
                            .into_iter()
                            .map(|row| (row.cs_id, row.id))
                            .collect();

                        // enumerate() would be OK here too, but involve conversions from usize
                        // to i32 within the map function.
                        let parent_inserts: Vec<_> = (0..(cs.parents.len() as i32))
                            .zip(cs.parents.iter())
                            .map(|(seq, parent)| {
                                // check_missing_rows should have ensured that all the IDs are
                                // present.
                                let parent_id = parent_map.get(&parent)
                                    .expect("check_missing_rows check failed");

                                ChangesetParentRow {
                                    cs_id: new_cs_row.id,
                                    parent_id: *parent_id,
                                    seq,
                                }
                            })
                            .collect();
                        insert_into(csparents::table)
                            .values(&parent_inserts)
                            .execute(&*connection)?;
                        Ok(())
                    })
                });

                future::result(txn_result).boxify()
            }
        }
    }
}

impl_changesets!(MysqlChangesets);
impl_changesets!(SqliteChangesets);

fn changeset_query<DB>(
    repo_id: RepositoryId,
    cs_id: HgChangesetId,
) -> changesets::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgChangesetIdSql>,
{
    changesets::table
        .filter(changesets::repo_id.eq(repo_id))
        .filter(changesets::cs_id.eq(cs_id))
        .limit(1)
        .into_boxed()
}

#[inline]
fn map_add_result(result: result::Result<usize, DieselError>) -> Result<()> {
    match result {
        Ok(_rows) => Ok(()),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            Err(ErrorKind::DuplicateChangeset.into())
        }
        Err(err) => Err(err.into()),
    }
}

fn check_missing_rows(
    expected: &[HgChangesetId],
    actual: &[ChangesetRow],
) -> result::Result<(), ErrorKind> {
    // Could just count the number here and report an error if any are missing, but the reporting
    // wouldn't be as nice.
    let expected_set: HashSet<_> = expected.iter().collect();
    let actual_set: HashSet<_> = actual.iter().map(|row| &row.cs_id).collect();
    let diff = &expected_set - &actual_set;
    if diff.is_empty() {
        Ok(())
    } else {
        Err(ErrorKind::MissingParents(
            diff.into_iter().map(|cs_id| *cs_id).collect(),
        ))
    }
}
