/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use context::CoreContext;
use context::PerfCounterType;
use filenodes::FilenodeResult;
use filenodes::PreparedFilenode;
use futures::future;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::RepoPath;
use mononoke_types::RepositoryId;
use path_hash::PathBytes;
use path_hash::PathHash;
use path_hash::PathHashBytes;
use sql::queries;
use sql::Connection;
use stats::prelude::*;
use std::collections::HashSet;
use thiserror::Error as DeriveError;
use tunables::tunables;

define_stats! {
    prefix = "mononoke.filenodes";
    adds: timeseries(Rate, Sum, Count),
    adds_disabled: timeseries(Rate, Sum, Count),
}

#[derive(Debug, Eq, DeriveError, PartialEq)]
pub enum ErrorKind {
    #[error("Invalid copy: {0:?} copied from {1:?}")]
    InvalidCopy(RepoPath, RepoPath),
}

pub struct FilenodesWriter {
    chunk_size: usize,
    write_connections: Vec<Connection>,
    read_connections: Vec<Connection>,
}

impl FilenodesWriter {
    pub fn new(
        chunk_size: usize,
        write_connections: Vec<Connection>,
        read_connections: Vec<Connection>,
    ) -> Self {
        Self {
            chunk_size,
            write_connections,
            read_connections,
        }
    }

    pub async fn insert_filenodes(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        filenodes: Vec<PreparedFilenode>,
        replace: bool,
    ) -> Result<FilenodeResult<()>, Error> {
        STATS::adds.add_value(filenodes.len() as i64);

        let shard_count = self.write_connections.len();

        let mut groups = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            groups.push(Vec::new());
        }

        for filenode in filenodes {
            let pwh = PathHash::from_repo_path(&filenode.path);
            let shard_number = pwh.shard_number(shard_count);
            groups[shard_number].push((pwh, filenode));
        }

        let futs = groups
            .into_iter()
            .enumerate()
            .map(|(shard_number, group)| {
                self.insert_filenode_group(ctx, repo_id, shard_number, group, replace)
            })
            .collect::<Vec<_>>();

        let res = future::try_join_all(futs).await?;

        for r in res {
            if let FilenodeResult::Disabled = r {
                return Ok(FilenodeResult::Disabled);
            }
        }

        Ok(FilenodeResult::Present(()))
    }

    async fn insert_filenode_group(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        shard_number: usize,
        filenodes: Vec<(PathHash, PreparedFilenode)>,
        replace: bool,
    ) -> Result<FilenodeResult<()>, Error> {
        if tunables().get_filenodes_disabled() {
            STATS::adds_disabled.add_value(1);
            return Ok(FilenodeResult::Disabled);
        }

        for chunk in filenodes.chunks(self.chunk_size) {
            let read_conn = &self.read_connections[shard_number];
            let write_conn = &self.write_connections[shard_number];
            ensure_paths_exists(ctx, read_conn, write_conn, repo_id, chunk)
                .await
                .context("Error ensuring filenode paths exist")?;
            insert_filenodes(ctx, write_conn, repo_id, chunk, replace)
                .await
                .context("Error inserting filenodes")?;
        }

        Ok(FilenodeResult::Present(()))
    }
}

async fn ensure_paths_exists(
    ctx: &CoreContext,
    read_conn: &Connection,
    write_conn: &Connection,
    repo_id: RepositoryId,
    filenodes: &[(PathHash, PreparedFilenode)],
) -> Result<(), Error> {
    let path_hashes = filenodes
        .iter()
        .map(|(pwh, _)| pwh.hash.clone())
        .collect::<Vec<_>>();

    ctx.perf_counters()
        .increment_counter(PerfCounterType::SqlReadsMaster);
    let mut paths_present = SelectAllPaths::query(read_conn, &repo_id, &path_hashes[..])
        .await?
        .into_iter()
        .map(|r| r.0)
        .collect::<HashSet<_>>();

    let mut paths_to_insert = filenodes
        .iter()
        .filter_map(|(pwh, _)| {
            if paths_present.insert(pwh.path_bytes.clone()) {
                Some((&repo_id, &pwh.path_bytes, &pwh.hash))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // If you have two concurrent INSERT OR IGNORE queries happening with the same rows, but in
    // different order, they will deadlock. Sorting the rows in each of our INSERT OR IGNORE
    // queries solves that. So we do it here.
    paths_to_insert.sort();

    ctx.perf_counters()
        .increment_counter(PerfCounterType::SqlWrites);
    InsertPaths::query(write_conn, &paths_to_insert[..])
        .await
        .with_context(|| format!("Error inserting {} filenode paths", paths_to_insert.len()))?;

    Ok(())
}

async fn insert_filenodes(
    ctx: &CoreContext,
    write_conn: &Connection,
    repo_id: RepositoryId,
    filenodes: &[(PathHash, PreparedFilenode)],
    replace: bool,
) -> Result<(), Error> {
    let mut filenode_rows = Vec::new();
    let mut copydata_rows = Vec::new();

    for (ph, filenode) in filenodes {
        filenode_rows.push((
            &repo_id,
            &ph.hash,
            ph.sql_is_tree(),
            &filenode.info.filenode,
            &filenode.info.linknode,
            &filenode.info.p1,
            &filenode.info.p2,
            if filenode.info.copyfrom.is_some() {
                &1i8
            } else {
                &0i8
            },
        ));

        if let Some(ref copyinfo) = filenode.info.copyfrom {
            let (ref frompath, ref fromnode) = copyinfo;
            let from_pwh = PathHash::from_repo_path(frompath);
            if from_pwh.is_tree != ph.is_tree {
                let e = ErrorKind::InvalidCopy(filenode.path.clone(), frompath.clone());
                return Err(e.into());
            }
            copydata_rows.push((
                &repo_id,
                &ph.hash,
                &filenode.info.filenode,
                ph.sql_is_tree(),
                from_pwh.hash,
                fromnode,
            ));
        }
    }

    let mut copydata_rows = copydata_rows
        .iter()
        .map(
            |&(repo_id, tohash, tonode, is_tree, ref fromhash, fromnode)| {
                (repo_id, tohash, tonode, is_tree, fromhash, fromnode)
            },
        )
        .collect::<Vec<_>>();

    // See above for why we sort.
    filenode_rows.sort();
    copydata_rows.sort();

    ctx.perf_counters()
        .increment_counter(PerfCounterType::SqlWrites);
    if replace {
        ReplaceFilenodes::query(write_conn, &filenode_rows[..]).await?;
    } else {
        InsertFilenodes::query(write_conn, &filenode_rows[..]).await?;
    }

    if !copydata_rows.is_empty() {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        InsertFixedcopyinfo::query(write_conn, &copydata_rows[..]).await?;
    }

    Ok(())
}

queries! {
    write InsertPaths(values: (repo_id: RepositoryId, path: PathBytes, path_hash: PathHashBytes)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO paths (repo_id, path, path_hash) VALUES {values}"
    }

    read SelectAllPaths(repo_id: RepositoryId, >list path_hashes: PathHashBytes) -> (PathBytes) {
        "SELECT path
         FROM paths
         WHERE paths.repo_id = {repo_id}
           AND paths.path_hash in {path_hashes}"
    }

    write InsertFilenodes(values: (
        repo_id: RepositoryId,
        path_hash: PathHashBytes,
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

    write ReplaceFilenodes(values: (
        repo_id: RepositoryId,
        path_hash: PathHashBytes,
        is_tree: i8,
        filenode: HgFileNodeId,
        linknode: HgChangesetId,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        has_copyinfo: i8,
    )) {
        none,
        "REPLACE INTO filenodes (
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
        topath_hash: PathHashBytes,
        tonode: HgFileNodeId,
        is_tree: i8,
        frompath_hash: PathHashBytes,
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
}
