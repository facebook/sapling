// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from, never_type)]

extern crate db;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;

extern crate filenodes;
extern crate futures_ext;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate stats;
extern crate tokio;

use db::{get_connection_params, ConnectionParams, InstanceRequirement, ProxyRequirement};
use diesel::{insert_or_ignore_into, Connection, SqliteConnection};
use diesel::backend::Backend;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::sql_types::HasSqlType;
use failure::{Error, Result, ResultExt};
use filenodes::{FilenodeInfo, Filenodes};
use futures::{Future, Stream};
use futures_ext::{asynchronize, BoxFuture, BoxStream};
use mercurial_types::{HgFileNodeId, RepoPath, RepositoryId};
use mercurial_types::sql_types::HgFileNodeIdSql;
use stats::Timeseries;

use std::sync::{Arc, Mutex, MutexGuard};

mod common;
mod errors;
mod models;
mod schema;

use errors::ErrorKind;

pub const DEFAULT_INSERT_CHUNK_SIZE: usize = 100;

define_stats! {
    prefix = "mononoke.filenodes";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    range_gets: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
}

#[derive(Clone)]
pub struct SqliteFilenodes {
    connection: Arc<Mutex<SqliteConnection>>,
    insert_chunk_size: usize,
}

impl SqliteFilenodes {
    /// Open a SQLite database. This is synchronous because the SQLite backend hits local
    /// disk or memory.
    pub fn open<P: AsRef<str>>(path: P, insert_chunk_size: usize) -> Result<Self> {
        let path = path.as_ref();
        let conn = SqliteConnection::establish(path)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn)),
            insert_chunk_size,
        })
    }

    fn create_tables(&mut self) -> Result<()> {
        let up_query = include_str!("../schemas/sqlite-filenodes.sql");

        self.connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(())
    }

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P, insert_chunk_size: usize) -> Result<Self> {
        let mut changesets = Self::open(path, insert_chunk_size)?;

        changesets.create_tables()?;

        Ok(changesets)
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P, insert_chunk_size: usize) -> Result<Self> {
        let mut filenodes = Self::open(path, insert_chunk_size)?;

        let _ = filenodes.create_tables();

        Ok(filenodes)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Self::create(":memory:", DEFAULT_INSERT_CHUNK_SIZE)
    }

    pub fn get_conn(&self) -> ::std::result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }

    pub fn get_master_conn(&self) -> ::std::result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }
}

#[derive(Clone)]
pub struct MysqlFilenodes {
    pool: Pool<ConnectionManager<MysqlConnection>>,
    master_pool: Pool<ConnectionManager<MysqlConnection>>,
    insert_chunk_size: usize,
}

impl MysqlFilenodes {
    pub fn open(db_address: &str, insert_chunk_size: usize) -> Result<Self> {
        let local_connection_params = get_connection_params(
            db_address.to_string(),
            InstanceRequirement::Closest,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;

        let master_connection_params = get_connection_params(
            db_address.to_string(),
            InstanceRequirement::Master,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;

        Self::open_with_params(
            &local_connection_params,
            &master_connection_params,
            insert_chunk_size,
        )
    }

    fn open_with_params(
        local_connection_params: &ConnectionParams,
        master_connection_params: &ConnectionParams,
        insert_chunk_size: usize,
    ) -> Result<Self> {
        let local_url = local_connection_params.to_diesel_url()?;
        let master_url = master_connection_params.to_diesel_url()?;

        let pool = Pool::builder()
            .max_size(10)
            .min_idle(Some(1))
            .build(ConnectionManager::new(local_url.clone()))?;
        let master_pool = Pool::builder()
            .max_size(1)
            .min_idle(Some(1))
            .build(ConnectionManager::new(master_url.clone()))?;
        Ok(Self {
            pool,
            master_pool,
            insert_chunk_size,
        })
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        let params = db::create_test_db(prefix)?;
        Self::create(&params)
    }

    fn create(params: &ConnectionParams) -> Result<Self> {
        let filenodes = Self::open_with_params(params, params, DEFAULT_INSERT_CHUNK_SIZE)?;

        let up_query = include_str!("../schemas/mysql-filenodes.sql");
        filenodes.master_pool.get()?.batch_execute(&up_query)?;

        Ok(filenodes)
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.pool.get().map_err(Error::from)
    }

    fn get_master_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.master_pool.get().map_err(Error::from)
    }
}

macro_rules! impl_filenodes {
    ($struct: ty, $connection: ty) => {
        impl Filenodes for $struct {
            fn add_filenodes(
                &self,
                filenodes: BoxStream<FilenodeInfo, Error>,
                repo_id: &RepositoryId,
            ) -> BoxFuture<(), Error> {
                let repo_id = *repo_id;
                let db = self.clone();
                let insert_chunk_size = self.insert_chunk_size;

                asynchronize(move || {
                    filenodes.chunks(insert_chunk_size).and_then(move |filenodes| {
                        STATS::adds.add_value(filenodes.len() as i64);
                        let connection = db.get_master_conn()?;
                        Self::do_insert(&connection, &filenodes, &repo_id)
                    })
                    .for_each(|()| Ok(()))
                    .from_err()
                })
            }

            fn get_filenode(
                &self,
                path: &RepoPath,
                filenode: &HgFileNodeId,
                repo_id: &RepositoryId,
            ) -> BoxFuture<Option<FilenodeInfo>, Error> {
                STATS::gets.add_value(1);
                let db = self.clone();
                let path = path.clone();
                let filenode = *filenode;
                let repo_id = *repo_id;

                asynchronize(move || {
                    let filenode_info = {
                        let conn = db.get_conn()?;
                        Self::actual_get(&*conn, &path, &filenode, &repo_id)?
                    };
                    if filenode_info.is_none() {
                        STATS::gets_master.add_value(1);
                        let conn = db.get_master_conn()?;
                        Self::actual_get(&*conn, &path, &filenode, &repo_id)
                    } else {
                        Ok(filenode_info)
                    }
                })
            }

            fn get_all_filenodes(
                &self,
                path: &RepoPath,
                repo_id: &RepositoryId,
            ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
                STATS::range_gets.add_value(1);
                let db = self.clone();
                let path = path.clone();
                let repo_id = *repo_id;

                asynchronize(move || {
                    let connection = db.get_conn()?;
                    let query = all_filenodes_query(&repo_id, &path);
                    let filenode_rows = query.load::<models::FilenodeRow>(&*connection)
                        .context(ErrorKind::FailRangeFetch(path.clone()))?;
                    let mut res = vec![];
                    for row in filenode_rows {
                        res.push(
                            Self::convert_to_filenode_info(&connection, &path, &repo_id, &row)?
                        );
                    }

                    Ok(res)
                })
            }
        }

        impl $struct {
            fn actual_get(
                conn: &$connection,
                path: &RepoPath,
                filenode: &HgFileNodeId,
                repo_id: &RepositoryId,
            ) -> Result<Option<FilenodeInfo>> {
                let query = single_filenode_query(&repo_id, &filenode, &path);
                let filenode_row = query.first::<models::FilenodeRow>(conn)
                    .optional()
                    .context(ErrorKind::FailFetchFilenode(filenode.clone(), path.clone()))?;

                match filenode_row {
                    Some(filenode_row) => {
                        let filenodeinfo = Self::convert_to_filenode_info(
                            conn,
                            &path,
                            &repo_id,
                            &filenode_row,
                        )?;
                        Ok(Some(filenodeinfo))
                    }
                    None => {
                        Ok(None)
                    },
                }
            }

            fn do_insert(
                connection: &$connection,
                filenodes: &Vec<FilenodeInfo>,
                repo_id: &RepositoryId,
            ) -> Result<()> {
                Self::ensure_paths_exists(&*connection, repo_id, &filenodes)?;

                Self::insert_filenodes(
                    &*connection,
                    &filenodes,
                    repo_id,
                )?;
                Ok(())
            }

            fn ensure_paths_exists(
                connection: &$connection,
                repo_id: &RepositoryId,
                filenodes: &Vec<FilenodeInfo>,
            ) -> Result<()> {
                let mut path_rows = vec![];
                for filenode in filenodes {
                    let (path_bytes, _) = convert_from_repo_path(&filenode.path);
                    let path_row = models::PathRow::new(repo_id, path_bytes);
                    path_rows.push(path_row);
                }

                insert_or_ignore_into(schema::paths::table)
                    .values(&path_rows)
                    .execute(&*connection)?;
                Ok(())
            }

            fn insert_filenodes(
                connection: &$connection,
                filenodes: &Vec<FilenodeInfo>,
                repo_id: &RepositoryId,
            ) -> Result<()> {
                let mut filenode_rows = vec![];
                let mut copydata_rows = vec![];
                for filenode in filenodes.clone() {
                    let (path_bytes, is_tree) = convert_from_repo_path(&filenode.path);
                    let filenode_row = models::FilenodeRow::new(
                        repo_id,
                        &path_bytes,
                        is_tree,
                        &filenode.filenode,
                        &filenode.linknode,
                        filenode.p1,
                        filenode.p2,
                        filenode.copyfrom.is_some(),
                    );
                    filenode_rows.push(filenode_row);
                    if let Some(copyinfo) = filenode.copyfrom {
                        let (frompath, fromnode) = copyinfo;
                        let (frompath_bytes, from_is_tree) = convert_from_repo_path(&frompath);
                        if from_is_tree != is_tree {
                            return Err(ErrorKind::InvalidCopy(filenode.path, frompath).into());
                        }
                        let copyinfo_row = models::FixedCopyInfoRow::new(
                            repo_id,
                            &frompath_bytes,
                            &fromnode,
                            is_tree,
                            &path_bytes,
                            &filenode.filenode,
                        );
                        copydata_rows.push(copyinfo_row);
                    }
                }

                // Do not try to insert filenode again - even if linknode is different!
                // That matches core hg behavior.
                insert_or_ignore_into(schema::filenodes::table)
                    .values(&filenode_rows)
                    .execute(&*connection)?;

                insert_or_ignore_into(schema::fixedcopyinfo::table)
                    .values(&copydata_rows)
                    .execute(&*connection)?;
                Ok(())
            }

            fn convert_to_filenode_info(
                connection: &$connection,
                path: &RepoPath,
                repo_id: &RepositoryId,
                row: &models::FilenodeRow,
            ) -> Result<FilenodeInfo> {
                let copyfrom = if row.has_copyinfo != 0 {
                    let copyfrom = Self::fetch_copydata(
                        &*connection,
                        &row.filenode,
                        &path,
                        &repo_id,
                    );
                    Some(
                        copyfrom.context(
                            ErrorKind::FailFetchCopydata(row.filenode.clone(), path.clone())
                        )?
                    )
                } else {
                    None
                };

                Ok(FilenodeInfo {
                    path: path.clone(),
                    filenode: row.filenode.clone(),
                    p1: row.p1,
                    p2: row.p2,
                    copyfrom,
                    linknode: row.linknode,
                })
            }

            fn fetch_copydata(
                connection: &$connection,
                filenode: &HgFileNodeId,
                path: &RepoPath,
                repo_id: &RepositoryId,
            ) -> Result<(RepoPath, HgFileNodeId)> {
                let is_tree = match path {
                    &RepoPath::RootPath | &RepoPath::DirectoryPath(_) => true,
                    &RepoPath::FilePath(_) => false,
                };

                let copyinfoquery = copyinfo_query(repo_id, filenode, path);

                let copydata_row =
                    copyinfoquery.first::<models::FixedCopyInfoRow>(&*connection)
                    .optional()?;

                let copydata: Result<_>  = copydata_row.ok_or(
                        ErrorKind::CopydataNotFound(filenode.clone(), path.clone()).into()
                );
                let copydata = copydata?;
                let path_row = path_query(repo_id, &copydata.frompath_hash)
                    .first::<models::PathRow>(&*connection)
                    .optional()?;
                match path_row {
                    Some(path_row) => {
                        let frompath = convert_to_repo_path(&path_row.path, is_tree)?;
                        Ok((frompath, copydata.fromnode))
                    }
                    None => {
                        let err: Error = ErrorKind::PathNotFound(copydata.frompath_hash).into();
                        Err(err)
                    }
                }
            }
        }
    }
}

impl_filenodes!(MysqlFilenodes, MysqlConnection);
impl_filenodes!(SqliteFilenodes, SqliteConnection);

fn all_filenodes_query<DB>(
    repo_id: &RepositoryId,
    path: &RepoPath,
) -> schema::filenodes::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgFileNodeIdSql>,
{
    let (path_bytes, is_tree) = convert_from_repo_path(path);

    let path_hash = common::blake2_path_hash(&path_bytes);

    schema::filenodes::table
        .filter(schema::filenodes::repo_id.eq(*repo_id))
        .filter(schema::filenodes::path_hash.eq(path_hash.clone()))
        .filter(schema::filenodes::is_tree.eq(is_tree))
        .into_boxed()
}

fn single_filenode_query<DB>(
    repo_id: &RepositoryId,
    filenode: &HgFileNodeId,
    path: &RepoPath,
) -> schema::filenodes::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgFileNodeIdSql>,
{
    let (path_bytes, is_tree) = convert_from_repo_path(path);

    let path_hash = common::blake2_path_hash(&path_bytes);

    schema::filenodes::table
        .filter(schema::filenodes::repo_id.eq(*repo_id))
        .filter(schema::filenodes::filenode.eq(*filenode))
        .filter(schema::filenodes::path_hash.eq(path_hash.clone()))
        .filter(schema::filenodes::is_tree.eq(is_tree))
        .limit(1)
        .into_boxed()
}

fn copyinfo_query<DB>(
    repo_id: &RepositoryId,
    tonode: &HgFileNodeId,
    topath: &RepoPath,
) -> schema::fixedcopyinfo::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgFileNodeIdSql>,
{
    let (topath_bytes, is_tree) = convert_from_repo_path(topath);

    let topath_hash = common::blake2_path_hash(&topath_bytes);

    schema::fixedcopyinfo::table
        .filter(schema::fixedcopyinfo::repo_id.eq(*repo_id))
        .filter(schema::fixedcopyinfo::topath_hash.eq(topath_hash))
        .filter(schema::fixedcopyinfo::tonode.eq(*tonode))
        .filter(schema::fixedcopyinfo::is_tree.eq(is_tree))
        .limit(1)
        .into_boxed()
}

fn path_query<DB>(
    repo_id: &RepositoryId,
    path_hash: &Vec<u8>,
) -> schema::paths::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgFileNodeIdSql>,
{
    schema::paths::table
        .filter(schema::paths::repo_id.eq(*repo_id))
        .filter(schema::paths::path_hash.eq(path_hash.clone()))
        .limit(1)
        .into_boxed()
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, i32) {
    match path {
        &RepoPath::RootPath => (vec![], 1),
        &RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), 1),
        &RepoPath::FilePath(ref file) => (file.to_vec(), 0),
    }
}

fn convert_to_repo_path<B: AsRef<[u8]>>(path_bytes: B, is_tree: bool) -> Result<RepoPath> {
    if is_tree {
        RepoPath::dir(path_bytes.as_ref())
    } else {
        RepoPath::file(path_bytes.as_ref())
    }
}
