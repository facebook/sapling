// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate db;
#[macro_use]
extern crate diesel;
extern crate failure_ext as failure;
extern crate futures;

extern crate filenodes;
#[macro_use]
extern crate futures_ext;
extern crate mercurial_types;
extern crate mononoke_types;

use db::ConnectionParams;
use diesel::{insert_or_ignore_into, Connection, SqliteConnection};
use diesel::backend::Backend;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::HasSqlType;
use failure::{Error, Result};
use filenodes::{FilenodeInfo, Filenodes};
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use mercurial_types::{HgFileNodeId, RepoPath, RepositoryId};
use mercurial_types::sql_types::HgFileNodeIdSql;

use std::sync::{Arc, Mutex};

mod common;
mod models;
mod schema;

pub const DEFAULT_INSERT_CHUNK_SIZE: usize = 100;

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

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P) -> Result<Self> {
        let filenodes = Self::open(path, DEFAULT_INSERT_CHUNK_SIZE)?;

        let up_query = include_str!("../schemas/sqlite-filenodes.sql");
        filenodes
            .connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(filenodes)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Self::create(":memory:")
    }
}

pub struct MysqlFilenodes {
    connection: Arc<Mutex<MysqlConnection>>,
    insert_chunk_size: usize,
}

impl MysqlFilenodes {
    pub fn open(params: ConnectionParams, insert_chunk_size: usize) -> Result<Self> {
        let url = params.to_diesel_url()?;
        let conn = MysqlConnection::establish(&url)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn)),
            insert_chunk_size,
        })
    }

    pub fn create_test_db<P: AsRef<str>>(prefix: P) -> Result<Self> {
        let params = db::create_test_db(prefix)?;
        Self::create(params)
    }

    fn create(params: ConnectionParams) -> Result<Self> {
        let filenodes = Self::open(params, DEFAULT_INSERT_CHUNK_SIZE)?;

        let up_query = include_str!("../schemas/mysql-filenodes.sql");
        filenodes
            .connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(filenodes)
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
                let connection = self.connection.clone();
                filenodes.chunks(self.insert_chunk_size).and_then(move |filenodes| {
                    let connection = connection.lock().expect("poisoned lock");
                    Self::do_insert(&connection, &filenodes, &repo_id)
                })
                .for_each(|()| Ok(()))
                .boxify()
            }

            fn get_filenode(
                &self,
                path: &RepoPath,
                filenode: &HgFileNodeId,
                repo_id: &RepositoryId,
            ) -> BoxFuture<Option<FilenodeInfo>, Error> {
                let connection = self.connection.lock().expect("lock poisoned");

                let query = filenode_query(repo_id, filenode, path);
                let filenode_row = try_boxfuture!(
                    query.first::<models::FilenodeRow>(&*connection).optional());
                match filenode_row {
                    Some(filenode_row) => {
                        let filenodeinfo = FilenodeInfo {
                            path: path.clone(),
                            filenode: filenode.clone(),
                            p1: filenode_row.p1,
                            p2: filenode_row.p2,
                            copyfrom: None,
                            linknode: filenode_row.linknode,
                        };

                        future::ok::<_, Error>(Some(filenodeinfo)).from_err().boxify()
                    }
                    None => {
                        future::ok::<_, Error>(None).from_err().boxify()
                    }
                }
            }
        }

        impl $struct {
            // TODO(stash): add copyfrom support
            fn do_insert(
                connection: &$connection,
                filenodes: &Vec<FilenodeInfo>,
                repo_id: &RepositoryId,
            ) -> BoxFuture<(), Error> {
                let txnres = connection.transaction::<_, Error, _>(|| {
                    Self::ensure_paths_exists(&*connection, repo_id, &filenodes)?;

                    Self::insert_filenodes(
                        &*connection,
                        &filenodes,
                        repo_id,
                    )?;
                    Ok(())
                });
                future::result(txnres).from_err().boxify()
            }

            fn ensure_paths_exists(
                connection: &$connection,
                repo_id: &RepositoryId,
                filenodes: &Vec<FilenodeInfo>,
            ) -> Result<()> {
                let mut path_rows = vec![];
                for filenode in filenodes {
                    let (path_bytes, _) = convert_repo_path(&filenode.path);
                    let path_row = models::PathInsertRow::new(repo_id, path_bytes);
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
                for filenode in filenodes.clone() {
                    let (path_bytes, is_tree) = convert_repo_path(&filenode.path);
                    let filenode_row = models::FilenodeRow::new(
                        repo_id,
                        &path_bytes,
                        is_tree,
                        &filenode.filenode,
                        &filenode.linknode,
                        filenode.p1,
                        filenode.p2,
                    );
                    filenode_rows.push(filenode_row);
                }

                // Do not try to insert filenode again - even if linknode is different!
                // That matches core hg behavior.
                insert_or_ignore_into(schema::filenodes::table)
                    .values(&filenode_rows)
                    .execute(&*connection)?;
                Ok(())
            }
        }
    }
}

impl_filenodes!(MysqlFilenodes, MysqlConnection);
impl_filenodes!(SqliteFilenodes, SqliteConnection);

fn filenode_query<DB>(
    repo_id: &RepositoryId,
    filenode: &HgFileNodeId,
    path: &RepoPath,
) -> schema::filenodes::BoxedQuery<'static, DB>
where
    DB: Backend,
    DB: HasSqlType<HgFileNodeIdSql>,
{
    let (path_bytes, is_tree) = convert_repo_path(path);

    let path_hash = common::blake2_path_hash(&path_bytes);

    schema::filenodes::table
        .filter(schema::filenodes::repo_id.eq(*repo_id))
        .filter(schema::filenodes::filenode.eq(*filenode))
        .filter(schema::filenodes::path_hash.eq(path_hash.clone()))
        .filter(schema::filenodes::is_tree.eq(is_tree))
        .limit(1)
        .into_boxed()
}

fn convert_repo_path(path: &RepoPath) -> (Vec<u8>, i32) {
    match path {
        &RepoPath::RootPath => (vec![], 1),
        &RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), 1),
        &RepoPath::FilePath(ref file) => (file.to_vec(), 0),
    }
}
