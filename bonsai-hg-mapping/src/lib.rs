// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from, never_type)]

extern crate abomonation;
#[macro_use]
extern crate abomonation_derive;
extern crate asyncmemo;
extern crate db_conn;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;
extern crate tokio;

extern crate db;
extern crate futures_ext;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate stats;

use std::result;
use std::sync::{Arc, MutexGuard};

use asyncmemo::{Asyncmemo, Filler, Weight};
use db_conn::{MysqlConnInner, SqliteConnInner};
use diesel::{insert_into, MysqlConnection, SqliteConnection};
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use diesel::result::{DatabaseErrorKind, Error as DieselError};

use futures::Future;
use futures_ext::{asynchronize, BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, RepositoryId};
use mononoke_types::ChangesetId;
use stats::Timeseries;

mod errors;
mod models;
mod schema;

pub use errors::*;
use models::BonsaiHgMappingRow;
use schema::bonsai_hg_mapping;

define_stats! {
    prefix = "mononoke.bonsai-hg-mapping";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
}

#[derive(Abomonation, Clone, Debug, Eq, Hash, HeapSizeOf, PartialEq)]
pub struct BonsaiHgMappingEntry {
    pub repo_id: RepositoryId,
    pub hg_cs_id: HgChangesetId,
    pub bcs_id: ChangesetId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, HeapSizeOf)]
pub enum BonsaiOrHgChangesetId {
    Bonsai(ChangesetId),
    Hg(HgChangesetId),
}

impl From<ChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaiOrHgChangesetId::Bonsai(cs_id)
    }
}

impl From<HgChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: HgChangesetId) -> Self {
        BonsaiOrHgChangesetId::Hg(cs_id)
    }
}

pub trait BonsaiHgMapping: Send + Sync {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error>;

    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error>;

    fn get_hg_from_bonsai(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<HgChangesetId>, Error> {
        self.get(repo_id, cs_id.into())
            .map(|result| result.map(|entry| entry.hg_cs_id))
            .boxify()
    }

    fn get_bonsai_from_hg(
        &self,
        repo_id: RepositoryId,
        cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        self.get(repo_id, cs_id.into())
            .map(|result| result.map(|entry| entry.bcs_id))
            .boxify()
    }
}

impl BonsaiHgMapping for Arc<BonsaiHgMapping> {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        (**self).add(entry)
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        (**self).get(repo_id, cs_id)
    }
}

pub struct CachingBonsaiHgMapping {
    mapping: Arc<BonsaiHgMapping>,
    cache: asyncmemo::Asyncmemo<BonsaiHgMappingFiller>,
}

impl CachingBonsaiHgMapping {
    pub fn new(mapping: Arc<BonsaiHgMapping>, sizelimit: usize) -> Self {
        let cache = asyncmemo::Asyncmemo::with_limits(
            "bonsai-hg-mapping",
            BonsaiHgMappingFiller::new(mapping.clone()),
            std::usize::MAX,
            sizelimit,
        );
        Self { mapping, cache }
    }
}

impl BonsaiHgMapping for CachingBonsaiHgMapping {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        self.mapping.add(entry)
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        self.cache
            .get((repo_id, cs.into()))
            .then(|val| match val {
                Ok(val) => Ok(Some(val)),
                Err(Some(err)) => Err(err),
                Err(None) => Ok(None),
            })
            .boxify()
    }
}

pub struct BonsaiHgMappingFiller {
    mapping: Arc<BonsaiHgMapping>,
}

impl BonsaiHgMappingFiller {
    fn new(mapping: Arc<BonsaiHgMapping>) -> Self {
        BonsaiHgMappingFiller { mapping }
    }
}

impl Filler for BonsaiHgMappingFiller {
    type Key = (RepositoryId, BonsaiOrHgChangesetId);
    type Value = Box<Future<Item = BonsaiHgMappingEntry, Error = Option<Error>> + Send>;

    fn fill(&self, _cache: &Asyncmemo<Self>, &(ref repo_id, ref cs_id): &Self::Key) -> Self::Value {
        self.mapping
            .get(*repo_id, *cs_id)
            .map_err(|err| Some(err))
            .and_then(|res| match res {
                Some(val) => Ok(val),
                None => Err(None),
            })
            .boxify()
    }
}

impl Weight for BonsaiOrHgChangesetId {
    fn get_weight(&self) -> usize {
        match self {
            &BonsaiOrHgChangesetId::Bonsai(ref id) => id.get_weight(),
            &BonsaiOrHgChangesetId::Hg(ref id) => id.get_weight(),
        }
    }
}

impl Weight for BonsaiHgMappingEntry {
    fn get_weight(&self) -> usize {
        self.repo_id.get_weight() + self.hg_cs_id.get_weight() + self.bcs_id.get_weight()
    }
}

#[derive(Clone)]
pub struct SqliteBonsaiHgMapping {
    inner: SqliteConnInner,
}

impl SqliteBonsaiHgMapping {
    fn from(inner: SqliteConnInner) -> Self {
        Self { inner }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-bonsai-hg-mapping.sql")
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Ok(Self::from(SqliteConnInner::in_memory(
            Self::get_up_query(),
        )?))
    }

    pub fn open_or_create<P: AsRef<str>>(path: P) -> Result<Self> {
        Ok(Self::from(SqliteConnInner::open_or_create(
            path,
            Self::get_up_query(),
        )?))
    }

    fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        self.inner.get_conn()
    }
    fn get_master_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        self.inner.get_master_conn()
    }
}

#[derive(Clone)]
pub struct MysqlBonsaiHgMapping {
    inner: MysqlConnInner,
}

impl MysqlBonsaiHgMapping {
    fn from(inner: MysqlConnInner) -> Self {
        Self { inner }
    }

    pub fn open(db_address: &str) -> Result<Self> {
        Ok(Self::from(MysqlConnInner::open(db_address)?))
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/mysql-bonsai-hg-mapping.sql")
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        Ok(Self::from(MysqlConnInner::create_test_db(
            prefix,
            Self::get_up_query(),
        )?))
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_conn()
    }

    fn get_master_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_master_conn()
    }
}

/// Using a macro here is unfortunate, but it appears to be the only way to share this code
/// between SQLite and MySQL.
/// See https://github.com/diesel-rs/diesel/issues/882#issuecomment-300257476
macro_rules! impl_bonsai_hg_mapping {
    ($struct:ty, $connection:ty) => {
        impl BonsaiHgMapping for $struct {
            fn get(
                &self,
                repo_id: RepositoryId,
                cs_id: BonsaiOrHgChangesetId,
            ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
                STATS::gets.add_value(1);
                let db = self.clone();

                asynchronize(move || {
                    let result = {
                        let connection = db.get_conn()?;
                        Self::actual_get(&connection, repo_id, cs_id)?
                    };

                    if result.is_none() {
                        STATS::gets_master.add_value(1);
                        let connection = db.get_master_conn()?;
                        Self::actual_get(&connection, repo_id, cs_id)
                    } else {
                        Ok(result)
                    }
                })
            }

            fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
                STATS::adds.add_value(1);
                let db = self.clone();

                asynchronize(move || {
                    let connection = db.get_master_conn()?;
                    let BonsaiHgMappingEntry {
                        repo_id,
                        hg_cs_id,
                        bcs_id,
                    } = entry.clone();
                    let result = insert_into(bonsai_hg_mapping::table)
                        .values(BonsaiHgMappingRow {
                            repo_id,
                            hg_cs_id,
                            bcs_id,
                        })
                        .execute(&*connection);
                    match result {
                        Ok(_) => Ok(true),
                        Err(
                            err @ DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _),
                        ) => {
                            let entry_by_bcs =
                                Self::actual_get(&connection, repo_id, bcs_id.into())?;
                            let entry_by_hgcs =
                                Self::actual_get(&connection, repo_id, hg_cs_id.into())?;
                            match entry_by_bcs.or(entry_by_hgcs) {
                                Some(ref stored_entry) if stored_entry == &entry => Ok(false),
                                Some(stored_entry) => {
                                    Err(ErrorKind::ConflictingEntries(stored_entry.clone(), entry)
                                        .into())
                                }
                                _ => Err(err.into()),
                            }
                        }
                        Err(err) => Err(err.into()),
                    }
                })
            }
        }

        impl $struct {
            fn actual_get(
                connection: &$connection,
                repo_id: RepositoryId,
                cs_id: BonsaiOrHgChangesetId,
            ) -> Result<Option<BonsaiHgMappingEntry>> {
                let query = match cs_id {
                    BonsaiOrHgChangesetId::Bonsai(id) => bonsai_hg_mapping::table
                        .filter(bonsai_hg_mapping::repo_id.eq(repo_id))
                        .filter(bonsai_hg_mapping::bcs_id.eq(id))
                        .limit(1)
                        .into_boxed(),
                    BonsaiOrHgChangesetId::Hg(id) => bonsai_hg_mapping::table
                        .filter(bonsai_hg_mapping::repo_id.eq(repo_id))
                        .filter(bonsai_hg_mapping::hg_cs_id.eq(id))
                        .limit(1)
                        .into_boxed(),
                };

                query
                    .first::<BonsaiHgMappingRow>(connection)
                    .optional()
                    .map_err(failure::Error::from)
                    .and_then(|row| match row {
                        None => Ok(None),
                        Some(row) => {
                            let BonsaiHgMappingRow {
                                repo_id,
                                hg_cs_id,
                                bcs_id,
                            } = row;
                            Ok(Some(BonsaiHgMappingEntry {
                                repo_id,
                                hg_cs_id,
                                bcs_id,
                            }))
                        }
                    })
            }
        }
    };
}

impl_bonsai_hg_mapping!(MysqlBonsaiHgMapping, MysqlConnection);
impl_bonsai_hg_mapping!(SqliteBonsaiHgMapping, SqliteConnection);
