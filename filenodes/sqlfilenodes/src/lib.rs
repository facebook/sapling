// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(wait_until)]

#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate sql;
#[macro_use]
extern crate stats;
extern crate tokio;

extern crate filenodes;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate sql_ext;

mod errors;

use failure::prelude::*;
use futures::{Future, IntoFuture, Stream, future::join_all};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use sql::Connection;
use stats::Timeseries;

use filenodes::{FilenodeInfo, Filenodes};
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath, RepositoryId};
use mononoke_types::hash;
pub use sql_ext::SqlConstructors;

use errors::ErrorKind;

const DEFAULT_INSERT_CHUNK_SIZE: usize = 100;

pub struct SqlFilenodes {
    write_connection: Vec<Connection>,
    read_connection: Vec<Connection>,
    read_master_connection: Vec<Connection>,
}

define_stats! {
    prefix = "mononoke.filenodes";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    range_gets: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
}

queries! {
    write InsertPaths(values: (repo_id: RepositoryId, path: Vec<u8>, path_hash: Vec<u8>)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO paths (repo_id, path, path_hash) VALUES {values}"
    }

    write InsertFilenodes(values: (
        repo_id: RepositoryId,
        path_hash: Vec<u8>,
        is_tree: i8,
        filenode: HgFileNodeId,
        linknode: HgChangesetId,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        has_copyinfo: i8,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO filenodes (
            repo_id
            , path_hash
            , is_tree
            , filenode
            , linknode
            , p1
            , p2
            , has_copyinfo
        ) VALUES {values}"
    }

    write InsertFixedcopyinfo(values: (
        repo_id: RepositoryId,
        topath_hash: Vec<u8>,
        tonode: HgFileNodeId,
        is_tree: i8,
        frompath_hash: Vec<u8>,
        fromnode: HgFileNodeId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO fixedcopyinfo (
            repo_id
            , topath_hash
            , tonode
            , is_tree
            , frompath_hash
            , fromnode
        ) VALUES {values}"
    }

    read SelectFilenode(
        repo_id: RepositoryId,
        path_hash: Vec<u8>,
        is_tree: i8,
        filenode: HgFileNodeId
    ) -> (HgChangesetId, Option<HgFileNodeId>, Option<HgFileNodeId>, i8) {
        "SELECT linknode, p1, p2, has_copyinfo
         FROM filenodes
         WHERE repo_id = {repo_id}
           AND path_hash = {path_hash}
           AND is_tree = {is_tree}
           AND filenode = {filenode}
         LIMIT 1"
    }

    read SelectAllFilenodes(
        repo_id: RepositoryId,
        path_hash: Vec<u8>,
        is_tree: i8
    ) -> (HgFileNodeId, HgChangesetId, Option<HgFileNodeId>, Option<HgFileNodeId>, i8) {
        "SELECT filenode, linknode, p1, p2, has_copyinfo
         FROM filenodes
         WHERE repo_id = {repo_id}
           AND path_hash = {path_hash}
           AND is_tree = {is_tree}"
    }

    read SelectCopyinfo(
        repo_id: RepositoryId,
        topath_hash: Vec<u8>,
        tonode: HgFileNodeId,
        is_tree: i8,
    ) -> (Vec<u8>, HgFileNodeId) {
        "SELECT paths.path, fromnode
         FROM fixedcopyinfo
         JOIN paths
           ON fixedcopyinfo.repo_id = paths.repo_id
          AND fixedcopyinfo.frompath_hash = paths.path_hash
         WHERE fixedcopyinfo.repo_id = {repo_id}
           AND fixedcopyinfo.topath_hash = {topath_hash}
           AND fixedcopyinfo.tonode = {tonode}
           AND fixedcopyinfo.is_tree = {is_tree}
         LIMIT 1"
    }
}

impl SqlConstructors for SqlFilenodes {
    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection: vec![write_connection],
            read_connection: vec![read_connection],
            read_master_connection: vec![read_master_connection],
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-filenodes.sql")
    }
}

impl SqlFilenodes {
    pub fn with_sharded_myrouter(tier: impl ToString, port: u16, shard_count: usize) -> Self {
        let new = Self {
            write_connection: Vec::with_capacity(shard_count),
            read_connection: Vec::with_capacity(shard_count),
            read_master_connection: Vec::with_capacity(shard_count),
        };
        let shards = 1..=shard_count;

        shards.fold(new, |mut new, shard_id| {
            let mut builder = sql::myrouter::Builder::new();
            builder
                .tier(format!("{}.{}", tier.to_string(), shard_id))
                .port(port);

            builder.tie_break(sql::myrouter::TieBreak::SLAVE_FIRST);
            new.read_connection.push(builder.build_read_only());

            builder.service_type(sql::myrouter::ServiceType::MASTER);
            new.read_master_connection.push(builder.build_read_only());
            new.write_connection.push(builder.build_read_write());

            new
        })
    }
}

impl Filenodes for SqlFilenodes {
    fn add_filenodes(
        &self,
        filenodes: BoxStream<FilenodeInfo, Error>,
        repo_id: &RepositoryId,
    ) -> BoxFuture<(), Error> {
        cloned!(repo_id, self.write_connection);

        filenodes
            .chunks(DEFAULT_INSERT_CHUNK_SIZE)
            .and_then(move |filenodes| {
                STATS::adds.add_value(filenodes.len() as i64);

                let filenodes: Vec<_> = filenodes
                    .into_iter()
                    .map(|filenode| {
                        let pwh = PathWithHash::from_repo_path(&filenode.path);
                        (filenode, pwh)
                    })
                    .collect();

                ensure_paths_exists(&write_connection, &repo_id, &filenodes).and_then({
                    cloned!(write_connection);
                    move |()| insert_filenodes(&write_connection, &repo_id, &filenodes)
                })
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
        STATS::gets.add_value(1);
        cloned!(self.read_master_connection, path, filenode, repo_id);
        let pwh = PathWithHash::from_repo_path(&path);

        select_filenode(&self.read_connection, &path, &filenode, &pwh, &repo_id)
            .and_then(move |maybe_filenode_info| match maybe_filenode_info {
                Some(filenode_info) => Ok(Some(filenode_info)).into_future().boxify(),
                None => {
                    STATS::gets_master.add_value(1);
                    select_filenode(&read_master_connection, &path, &filenode, &pwh, &repo_id)
                }
            })
            .boxify()
    }

    fn get_all_filenodes(
        &self,
        path: &RepoPath,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        STATS::range_gets.add_value(1);
        cloned!(self.read_connection, path, repo_id);
        let pwh = PathWithHash::from_repo_path(&path);

        SelectAllFilenodes::query(
            &read_connection[pwh.shard_number(read_connection.len())],
            &repo_id,
            &pwh.hash,
            &pwh.is_tree,
        ).chain_err(ErrorKind::FailRangeFetch(path.clone()))
            .from_err()
            .and_then(move |filenode_rows| {
                let mut futs = vec![];
                for (filenode, linknode, p1, p2, has_copyinfo) in filenode_rows {
                    futs.push(convert_to_filenode_info(
                        &read_connection,
                        path.clone(),
                        filenode,
                        &pwh,
                        repo_id,
                        linknode,
                        p1,
                        p2,
                        has_copyinfo,
                    ))
                }

                join_all(futs)
            })
            .boxify()
    }
}

fn ensure_paths_exists(
    connections: &Vec<Connection>,
    repo_id: &RepositoryId,
    filenodes: &Vec<(FilenodeInfo, PathWithHash)>,
) -> impl Future<Item = (), Error = Error> {
    let mut path_rows: Vec<Vec<_>> = connections.iter().map(|_| Vec::new()).collect();
    for &(_, ref pwh) in filenodes {
        path_rows[pwh.shard_number(connections.len())].push((repo_id, &pwh.path_bytes, &pwh.hash));
    }

    let futures: Vec<_> = connections
        .iter()
        .enumerate()
        .filter_map(|(shard, connection)| {
            if path_rows[shard].len() != 0 {
                Some(InsertPaths::query(&connection.clone(), &path_rows[shard]))
            } else {
                None
            }
        })
        .collect();
    join_all(futures).map(|_| ())
}

fn insert_filenodes(
    connections: &Vec<Connection>,
    repo_id: &RepositoryId,
    filenodes: &Vec<(FilenodeInfo, PathWithHash)>,
) -> impl Future<Item = (), Error = Error> {
    let mut filenode_rows: Vec<Vec<_>> = connections.iter().map(|_| Vec::new()).collect();
    let mut copydata_rows: Vec<Vec<_>> = connections.iter().map(|_| Vec::new()).collect();
    for &(ref filenode, ref pwh) in filenodes {
        filenode_rows[pwh.shard_number(connections.len())].push((
            repo_id,
            &pwh.hash,
            &pwh.is_tree,
            &filenode.filenode,
            &filenode.linknode,
            &filenode.p1,
            &filenode.p2,
            if filenode.copyfrom.is_some() {
                &1i8
            } else {
                &0i8
            },
        ));

        if let Some(ref copyinfo) = filenode.copyfrom {
            let (ref frompath, ref fromnode) = copyinfo;
            let from_pwh = PathWithHash::from_repo_path(frompath);
            if from_pwh.is_tree != pwh.is_tree {
                return Err(ErrorKind::InvalidCopy(filenode.path.clone(), frompath.clone()).into())
                    .into_future()
                    .left_future();
            }
            copydata_rows[pwh.shard_number(connections.len())].push((
                repo_id,
                &pwh.hash,
                &filenode.filenode,
                &pwh.is_tree,
                from_pwh.hash,
                fromnode,
            ));
        }
    }

    let copydata_rows: Vec<Vec<_>> = copydata_rows
        .iter()
        .map(|shard| {
            shard
                .iter()
                .map(
                    |&(repo_id, tohash, tonode, is_tree, ref fromhash, fromnode)| {
                        (repo_id, tohash, tonode, is_tree, fromhash, fromnode)
                    },
                )
                .collect()
        })
        .collect();

    let copyinfo_futures: Vec<_> = connections
        .iter()
        .enumerate()
        .filter_map(|(shard, connection)| {
            if copydata_rows[shard].len() != 0 {
                Some(InsertFixedcopyinfo::query(
                    &connection.clone(),
                    &copydata_rows[shard],
                ))
            } else {
                None
            }
        })
        .collect();
    let filenode_futures: Vec<_> = connections
        .iter()
        .enumerate()
        .filter_map(|(shard, connection)| {
            if filenode_rows[shard].len() != 0 {
                Some(InsertFilenodes::query(
                    &connection.clone(),
                    &filenode_rows[shard],
                ))
            } else {
                None
            }
        })
        .collect();

    join_all(filenode_futures)
        .join(join_all(copyinfo_futures))
        .map(|_| ())
        .right_future()
}

fn select_filenode(
    connections: &Vec<Connection>,
    path: &RepoPath,
    filenode: &HgFileNodeId,
    pwh: &PathWithHash,
    repo_id: &RepositoryId,
) -> BoxFuture<Option<FilenodeInfo>, Error> {
    let connection = &connections[pwh.shard_number(connections.len())];
    cloned!(connections, path, filenode, pwh, repo_id);

    SelectFilenode::query(connection, &repo_id, &pwh.hash, &pwh.is_tree, &filenode)
        .chain_err(ErrorKind::FailFetchFilenode(filenode.clone(), path.clone()))
        .from_err()
        .and_then({
            move |rows| match rows.into_iter().next() {
                Some((linknode, p1, p2, has_copyinfo)) => convert_to_filenode_info(
                    &connections,
                    path,
                    filenode,
                    &pwh,
                    repo_id,
                    linknode,
                    p1,
                    p2,
                    has_copyinfo,
                ).map(Some)
                    .boxify(),
                None => Ok(None).into_future().boxify(),
            }
        })
        .boxify()
}

fn select_copydata(
    connections: &Vec<Connection>,
    path: &RepoPath,
    filenode: &HgFileNodeId,
    pwh: &PathWithHash,
    repo_id: &RepositoryId,
) -> BoxFuture<(RepoPath, HgFileNodeId), Error> {
    let connection = &connections[pwh.shard_number(connections.len())];
    SelectCopyinfo::query(connection, repo_id, &pwh.hash, filenode, &pwh.is_tree)
        .and_then({
            cloned!(path, filenode);
            move |maybe_copyinfo_row| {
                maybe_copyinfo_row
                    .into_iter()
                    .next()
                    .ok_or(ErrorKind::CopydataNotFound(filenode, path).into())
            }
        })
        .and_then({
            cloned!(pwh.is_tree);
            move |(path, fromnode)| Ok((convert_to_repo_path(&path, is_tree)?, fromnode))
        })
        .chain_err(ErrorKind::FailFetchCopydata(filenode.clone(), path.clone()))
        .from_err()
        .boxify()
}

fn convert_to_filenode_info(
    connections: &Vec<Connection>,
    path: RepoPath,
    filenode: HgFileNodeId,
    pwh: &PathWithHash,
    repo_id: RepositoryId,
    linknode: HgChangesetId,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    has_copyinfo: i8,
) -> impl Future<Item = FilenodeInfo, Error = Error> {
    let copydata = if has_copyinfo != 0 {
        select_copydata(connections, &path, &filenode, &pwh, &repo_id)
            .map(Some)
            .boxify()
    } else {
        Ok(None).into_future().boxify()
    };

    copydata.map(move |copydata| FilenodeInfo {
        path,
        filenode,
        p1,
        p2,
        copyfrom: copydata,
        linknode,
    })
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, i8) {
    match path {
        &RepoPath::RootPath => (vec![], 1),
        &RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), 1),
        &RepoPath::FilePath(ref file) => (file.to_vec(), 0),
    }
}

fn convert_to_repo_path<B: AsRef<[u8]>>(path_bytes: B, is_tree: i8) -> Result<RepoPath> {
    if is_tree != 0 {
        RepoPath::dir(path_bytes.as_ref())
    } else {
        RepoPath::file(path_bytes.as_ref())
    }
}

#[derive(Clone)]
struct PathWithHash {
    path_bytes: Vec<u8>,
    is_tree: i8,
    hash: Vec<u8>,
}

impl PathWithHash {
    fn from_repo_path(path: &RepoPath) -> Self {
        let (path_bytes, is_tree) = convert_from_repo_path(path);

        let hash = {
            let mut hash_content = hash::Context::new("path".as_bytes());
            hash_content.update(&path_bytes);
            Vec::from(hash_content.finish().as_ref())
        };

        Self {
            path_bytes,
            is_tree,
            hash,
        }
    }

    fn shard_number(&self, shard_count: usize) -> usize {
        // We don't need crypto strength here - we're just turning a potentially large hash into
        // a shard number.
        let raw_shard_number = self.hash
            .iter()
            .fold(0usize, |hash, byte| hash.rotate_left(8) ^ (*byte as usize));

        raw_shard_number % shard_count
    }
}
