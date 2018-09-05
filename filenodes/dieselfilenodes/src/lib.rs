// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from, never_type)]

extern crate db;
extern crate db_conn;
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

use db::{get_connection_params, InstanceRequirement, ProxyRequirement};
use db_conn::{MysqlConnInner, SqliteConnInner};
use diesel::{insert_or_ignore_into, SqliteConnection};
use diesel::backend::Backend;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use diesel::sql_types::HasSqlType;
use failure::{Error, Result, ResultExt};
use filenodes::{FilenodeInfo, Filenodes, blake2_path_hash};
use futures::{Future, Stream};
use futures_ext::{asynchronize, BoxFuture, BoxStream, FutureExt};
use mercurial_types::{HgFileNodeId, RepoPath, RepositoryId};
use mercurial_types::sql_types::HgFileNodeIdSql;
use stats::Timeseries;

use std::result;
use std::sync::MutexGuard;

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
    inner: SqliteConnInner,
    insert_chunk_size: usize,
}

impl SqliteFilenodes {
    fn from(inner: SqliteConnInner, insert_chunk_size: usize) -> SqliteFilenodes {
        SqliteFilenodes {
            inner,
            insert_chunk_size,
        } // one true constructor
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-filenodes.sql")
    }

    pub fn in_memory() -> Result<Self> {
        let up_query = Self::get_up_query();
        Ok(Self::from(
            SqliteConnInner::in_memory(up_query)?,
            DEFAULT_INSERT_CHUNK_SIZE,
        ))
    }

    /// Open a SQLite database, and create the tables if they are missing
    pub fn open_or_create<P: AsRef<str>>(path: P, insert_chunk_size: usize) -> Result<Self> {
        Ok(Self::from(
            SqliteConnInner::open_or_create(&path, Self::get_up_query())?,
            insert_chunk_size,
        ))
    }

    pub fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        self.inner.get_conn()
    }
    pub fn get_master_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        self.inner.get_master_conn()
    }
}

#[derive(Clone)]
pub struct MysqlFilenodes {
    inner: MysqlConnInner,
    insert_chunk_size: usize,
}

impl MysqlFilenodes {
    fn from(inner: MysqlConnInner, insert_chunk_size: usize) -> MysqlFilenodes {
        MysqlFilenodes {
            inner,
            insert_chunk_size,
        } // one true constructor
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/mysql-filenodes.sql")
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        Ok(Self::from(
            MysqlConnInner::create_test_db(prefix, Self::get_up_query())?,
            DEFAULT_INSERT_CHUNK_SIZE,
        ))
    }

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

        Ok(Self::from(
            MysqlConnInner::open_with_params(&local_connection_params, &master_connection_params)?,
            insert_chunk_size,
        ))
    }

    pub fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_conn()
    }
    pub fn get_master_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_master_conn()
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

                filenodes.chunks(insert_chunk_size)
                    .and_then(move |filenodes| {
                        STATS::adds.add_value(filenodes.len() as i64);
                        asynchronize({
                            let db = db.clone();
                            move || {
                                let connection = db.get_master_conn()?;
                                Self::do_insert(&connection, &filenodes, &repo_id)
                            }
                        })
                    })
                    .for_each(|()| Ok(()))
                    .from_err()
                    .boxify()
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
                }).boxify()
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
                }).boxify()
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

    let path_hash = Vec::from(blake2_path_hash(&path_bytes).as_ref());

    schema::filenodes::table
        .filter(schema::filenodes::repo_id.eq(*repo_id))
        .filter(schema::filenodes::path_hash.eq(path_hash))
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

    let path_hash = Vec::from(blake2_path_hash(&path_bytes).as_ref());

    schema::filenodes::table
        .filter(schema::filenodes::repo_id.eq(*repo_id))
        .filter(schema::filenodes::filenode.eq(*filenode))
        .filter(schema::filenodes::path_hash.eq(path_hash))
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

    let topath_hash = Vec::from(blake2_path_hash(&topath_bytes).as_ref());

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
