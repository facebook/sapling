/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::{CoreContext, PerfCounterType};
use faster_hex::hex_encode;
use futures_preview::{
    compat::Future01CompatExt,
    future::{self, Future},
};
use itertools::Itertools;
use mercurial_types::{HgChangesetId, HgFileNodeId};
use mononoke_types::{RepoPath, RepositoryId};
use sql::{queries, Connection};
use stats::prelude::*;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::Duration;
use thiserror::Error as DeriveError;
use tokio_preview::time::timeout;

use filenodes::FilenodeInfo;

use crate::connections::{AcquireReason, Connections};
use crate::local_cache::{CacheKey, LocalCache, TrackedLocalCache};
use crate::remote_cache::RemoteCache;
use crate::structs::{PartialFilenode, PartialHistory, PathBytes, PathHashBytes, PathWithHash};

define_stats! {
    prefix = "mononoke.filenodes";
    gets: timeseries(Sum),
    gets_master: timeseries(Sum),
    range_gets: timeseries(Sum),
    path_gets: timeseries(Sum),
    get_local_cache_misses: timeseries(Sum),
    range_local_cache_misses: timeseries(Sum),
    paths_local_cache_misses: timeseries(Sum),
    remote_cache_timeouts: timeseries(Sum),
    sql_timeouts: timeseries(Sum),
}

// Both of these are pretty convervative, and collected experimentally. They're here to ensure one
// bad query doesn't lock down an entire shard for an extended period of time.
const REMOTE_CACHE_TIMEOUT_MILLIS: u64 = 100;
const SQL_TIMEOUT_MILLIS: u64 = 5_000;

#[derive(Debug, DeriveError)]
pub enum ErrorKind {
    #[error("Internal error: path is not found: {0:?}")]
    PathNotFound(PathHashBytes),

    #[error("Internal error: fixedcopyinfo is missing for filenode: {0:?}")]
    FixedCopyInfoMissing(HgFileNodeId),

    #[error("Internal error: SQL error: {0:?}")]
    SqlError(Error),

    #[error("Internal error: SQL timeout")]
    SqlTimeout,

    #[error("Internal error: Path conversion failed: {0:?}")]
    PathConversionFailed(Error),
}

enum Selection<Partial, Full> {
    Partial(Partial),
    Full(Full),
}

type FilenodeRow = (
    HgFileNodeId,
    HgChangesetId,
    Option<HgFileNodeId>,
    Option<HgFileNodeId>,
    i8,
    Option<PathHashBytes>,
    Option<HgFileNodeId>,
);

fn filenode_cache_key(
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: &HgFileNodeId,
) -> CacheKey<PartialFilenode> {
    let mut v = vec![0; pwh.hash.0.len() * 2];
    hex_encode(pwh.hash.0.as_ref(), &mut v).expect("failed to hex encode");
    let key = format!("filenode.{}.{}.{}", repo_id.id(), filenode, unsafe {
        String::from_utf8_unchecked(v)
    });

    CacheKey {
        key,
        value: PhantomData,
    }
}

fn history_cache_key(repo_id: RepositoryId, pwh: &PathWithHash<'_>) -> CacheKey<PartialHistory> {
    let mut v = vec![0; pwh.hash.0.len() * 2];
    hex_encode(pwh.hash.0.as_ref(), &mut v).expect("failed to hex encode");
    let key = format!("history.{}.{}", repo_id.id(), unsafe {
        String::from_utf8_unchecked(v)
    });

    CacheKey {
        key,
        value: PhantomData,
    }
}

fn path_cache_key(repo_id: RepositoryId, hash: &PathHashBytes) -> CacheKey<PathBytes> {
    let mut v = vec![0; hash.0.len() * 2];
    hex_encode(hash.0.as_ref(), &mut v).expect("failed to hex encode");
    let key = format!("hash.{}.{}", repo_id.id(), unsafe {
        String::from_utf8_unchecked(v)
    });

    CacheKey {
        key,
        value: PhantomData,
    }
}

pub struct FilenodesReader {
    read_connections: Connections,
    read_master_connections: Connections,
    pub local_cache: LocalCache,
    pub remote_cache: RemoteCache,
}

impl FilenodesReader {
    pub fn new(
        read_connections: Vec<Connection>,
        read_master_connections: Vec<Connection>,
    ) -> Self {
        Self {
            read_connections: Connections::new(read_connections),
            read_master_connections: Connections::new(read_master_connections),
            local_cache: LocalCache::Noop,
            remote_cache: RemoteCache::Noop,
        }
    }

    pub async fn get_filenode(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        path: RepoPath,
        filenode: HgFileNodeId,
    ) -> Result<Option<FilenodeInfo>, Error> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let pwh = PathWithHash::from_repo_path(&path);

        match select_filenode(
            &self.local_cache,
            &self.remote_cache,
            &self.read_connections,
            repo_id,
            &pwh,
            filenode,
        )
        .await
        {
            Ok(Some(res)) => {
                return Ok(Some(res));
            }
            Ok(None) | Err(ErrorKind::FixedCopyInfoMissing(_)) => {
                // If the filenode wasn't found, or its copy info was missing, it might be present
                // on the master.
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        STATS::gets_master.add_value(1);

        let res = select_filenode(
            &self.local_cache,
            &self.remote_cache,
            &self.read_master_connections,
            repo_id,
            &pwh,
            filenode,
        )
        .await?;

        Ok(res)
    }

    pub async fn get_all_filenodes_for_path(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        path: RepoPath,
    ) -> Result<Vec<FilenodeInfo>, Error> {
        STATS::range_gets.add_value(1);

        let pwh = PathWithHash::from_repo_path(&path);

        let res = select_history(
            &self.local_cache,
            &self.remote_cache,
            &self.read_connections,
            repo_id,
            &pwh,
        )
        .await?;

        Ok(res)
    }
}

async fn select_filenode(
    local_cache: &LocalCache,
    remote_cache: &RemoteCache,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: HgFileNodeId,
) -> Result<Option<FilenodeInfo>, ErrorKind> {
    let local_cache = local_cache.tracked();

    let selection = select_partial_filenode(
        &local_cache,
        remote_cache,
        connections,
        repo_id,
        pwh,
        filenode,
    )
    .await?;

    let partial = match selection {
        Some(Selection::Partial(partial)) => partial,
        Some(Selection::Full(full)) => {
            return Ok(Some(full));
        }
        None => {
            return Ok(None);
        }
    };

    let ret = match fill_paths(connections, &local_cache, pwh, repo_id, vec![partial])
        .await?
        .into_iter()
        .next()
    {
        Some(ret) => ret,
        None => {
            return Ok(None);
        }
    };

    if local_cache.did_fill() {
        remote_cache.fill_filenode(repo_id, &pwh.path, filenode, ret.clone());
    }

    Ok(Some(ret))
}

async fn select_partial_filenode(
    local_cache: &TrackedLocalCache<'_>,
    remote_cache: &RemoteCache,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: HgFileNodeId,
) -> Result<Option<Selection<PartialFilenode, FilenodeInfo>>, ErrorKind> {
    let key = filenode_cache_key(repo_id, &pwh, &filenode);

    // Check our local cache first.
    if let Some(partial) = local_cache.get(&key) {
        return Ok(Some(Selection::Partial(partial)));
    }

    // Otherwise, request access to MySQL.
    let connection = connections.acquire(&pwh, AcquireReason::Filenodes).await;

    // Check the cache before dispatching any work: if we waited a long time to get the connection,
    // it's possible that the cache has been filled by now.
    if let Some(partial) = local_cache.get(&key) {
        return Ok(Some(Selection::Partial(partial)));
    }

    STATS::get_local_cache_misses.add_value(1);

    // Before hiting MySQL, check our caches.

    let from_remote_cache =
        enforce_remote_cache_timeout(remote_cache.get_filenode(repo_id, &pwh.path, filenode)).await;

    if let Some(full) = from_remote_cache {
        fill_full_filenode(local_cache.untracked(), &key, repo_id, &full);
        return Ok(Some(Selection::Full(full)));
    }

    let rows = enforce_sql_timeout(
        SelectFilenode::query(
            connection.as_ref(),
            &repo_id,
            &pwh.hash,
            &pwh.is_tree,
            &filenode,
        )
        .compat(),
    )
    .await?;

    match rows.into_iter().next() {
        Some(row) => {
            let ret = convert_row_to_partial_filenode(row)?;
            local_cache.fill(&key, &ret);
            Ok(Some(Selection::Partial(ret)))
        }
        None => Ok(None),
    }
}

async fn select_history(
    local_cache: &LocalCache,
    remote_cache: &RemoteCache,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
) -> Result<Vec<FilenodeInfo>, Error> {
    let local_cache = local_cache.tracked();

    let selection =
        select_partial_history(&local_cache, &remote_cache, &connections, repo_id, &pwh).await?;

    let partial = match selection {
        Selection::Partial(partial) => partial,
        Selection::Full(full) => {
            return Ok(full);
        }
    };

    let ret = fill_paths(&connections, &local_cache, &pwh, repo_id, partial.history).await?;

    if local_cache.did_fill() {
        remote_cache.fill_history(repo_id, &pwh.path, ret.clone());
    }

    Ok(ret)
}

async fn select_partial_history(
    local_cache: &TrackedLocalCache<'_>,
    remote_cache: &RemoteCache,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
) -> Result<Selection<PartialHistory, Vec<FilenodeInfo>>, ErrorKind> {
    let key = history_cache_key(repo_id, &pwh);

    if let Some(history) = local_cache.get(&key) {
        return Ok(Selection::Partial(history));
    }

    let connection = connections.acquire(&pwh, AcquireReason::History).await;

    if let Some(history) = local_cache.get(&key) {
        return Ok(Selection::Partial(history));
    }

    STATS::range_local_cache_misses.add_value(1);

    let from_remote_cache =
        enforce_remote_cache_timeout(remote_cache.get_history(repo_id, &pwh.path)).await;

    if let Some(full) = from_remote_cache {
        fill_full_history(local_cache.untracked(), &key, repo_id, &full[..]);
        return Ok(Selection::Full(full));
    }

    let rows = enforce_sql_timeout(
        SelectAllFilenodes::query(connection.as_ref(), &repo_id, &pwh.hash, &pwh.is_tree).compat(),
    )
    .await?;

    let history = rows
        .into_iter()
        .map(|row| convert_row_to_partial_filenode(row))
        .collect::<Result<Vec<PartialFilenode>, ErrorKind>>()?;

    // TODO: It'd be nice to have some eviction here.
    // TODO: It'd be nice to chain those.
    let history = PartialHistory { history };

    local_cache.fill(&key, &history);

    Ok(Selection::Partial(history))
}

fn convert_row_to_partial_filenode(row: FilenodeRow) -> Result<PartialFilenode, ErrorKind> {
    let (filenode, linknode, p1, p2, has_copyinfo, from_path_hash, from_node) = row;

    let copyfrom = if has_copyinfo == 0 {
        None
    } else {
        let from_path_hash =
            from_path_hash.ok_or_else(|| ErrorKind::FixedCopyInfoMissing(filenode))?;

        let from_node = from_node.ok_or_else(|| ErrorKind::FixedCopyInfoMissing(filenode))?;

        Some((from_path_hash, from_node))
    };

    let ret = PartialFilenode {
        filenode,
        p1,
        p2,
        copyfrom,
        linknode,
    };

    Ok(ret)
}

async fn fill_paths(
    connections: &Connections,
    local_cache: &TrackedLocalCache<'_>,
    pwh: &PathWithHash<'_>,
    repo_id: RepositoryId,
    rows: Vec<PartialFilenode>,
) -> Result<Vec<FilenodeInfo>, ErrorKind> {
    let path_hashes_to_fetch = rows
        .iter()
        .filter_map(|r| r.copyfrom.as_ref().map(|c| c.0.clone()));

    let path_hashes_to_paths =
        select_paths(connections, local_cache, repo_id, path_hashes_to_fetch).await?;

    let ret = rows
        .into_iter()
        .map(|partial| {
            let PartialFilenode {
                filenode,
                p1,
                p2,
                copyfrom,
                linknode,
            } = partial;

            let copyfrom = match copyfrom {
                Some((from_path_hash, from_node)) => {
                    let from_path = path_hashes_to_paths
                        .get(&from_path_hash)
                        .ok_or_else(|| ErrorKind::PathNotFound(from_path_hash.clone()))?
                        .clone();

                    let from_path = convert_to_repo_path(&from_path, pwh.is_tree)?;

                    Some((from_path, from_node))
                }
                None => None,
            };

            let ret = FilenodeInfo {
                path: pwh.path.clone(),
                filenode,
                p1,
                p2,
                copyfrom,
                linknode,
            };

            Result::<_, ErrorKind>::Ok(ret)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ret)
}

async fn select_paths<I: Iterator<Item = PathHashBytes>>(
    connections: &Connections,
    local_cache: &TrackedLocalCache<'_>,
    repo_id: RepositoryId,
    iter: I,
) -> Result<HashMap<PathHashBytes, PathBytes>, ErrorKind> {
    let futs = iter
        .group_by(|path_hash| PathWithHash::shard_number_by_hash(&path_hash, connections.len()))
        .into_iter()
        .map(|(shard_num, group)| {
            let group = group.collect::<Vec<_>>();

            STATS::path_gets.add_value(group.len() as i64);

            let mut output = HashMap::new();
            let group = local_cache.populate(repo_id, &mut output, group, path_cache_key);

            async move {
                let connection = connections
                    .acquire_by_shard_number(shard_num, AcquireReason::Paths)
                    .await;

                let group = local_cache.populate(repo_id, &mut output, group, path_cache_key);

                STATS::paths_local_cache_misses.add_value(group.len() as i64);

                if group.len() > 0 {
                    let paths = enforce_sql_timeout(
                        SelectPaths::query(connection.as_ref(), &repo_id, &group[..]).compat(),
                    )
                    .await?
                    .into_iter();

                    for (path_hash, path) in paths {
                        local_cache.fill(&path_cache_key(repo_id, &path_hash), &path);
                        output.insert(path_hash, path);
                    }
                }

                Result::<_, ErrorKind>::Ok(output)
            }
        })
        .collect::<Vec<_>>();

    let groups = future::try_join_all(futs).await?;

    let mut ret = HashMap::new();
    for group in groups {
        ret.extend(group);
    }

    Ok(ret)
}

fn convert_to_repo_path(path_bytes: &PathBytes, is_tree: i8) -> Result<RepoPath, ErrorKind> {
    if is_tree != 0 {
        RepoPath::dir(&path_bytes.0[..]).map_err(ErrorKind::PathConversionFailed)
    } else {
        RepoPath::file(&path_bytes.0[..]).map_err(ErrorKind::PathConversionFailed)
    }
}

fn extract_partial_filenode(
    local_cache: &LocalCache,
    repo_id: RepositoryId,
    info: &FilenodeInfo,
) -> PartialFilenode {
    let copyfrom = match &info.copyfrom {
        Some((path, filenode_id)) => {
            let pwh = PathWithHash::from_repo_path(&path);
            local_cache.fill(&path_cache_key(repo_id, &pwh.hash), &pwh.path_bytes);
            Some((pwh.hash, *filenode_id))
        }
        None => None,
    };

    PartialFilenode {
        filenode: info.filenode,
        p1: info.p1,
        p2: info.p2,
        copyfrom,
        linknode: info.linknode,
    }
}

fn fill_full_filenode(
    local_cache: &LocalCache,
    key: &CacheKey<PartialFilenode>,
    repo_id: RepositoryId,
    info: &FilenodeInfo,
) {
    let partial = extract_partial_filenode(local_cache, repo_id, info);
    local_cache.fill(key, &partial);
}

fn fill_full_history(
    local_cache: &LocalCache,
    key: &CacheKey<PartialHistory>,
    repo_id: RepositoryId,
    history: &[FilenodeInfo],
) {
    let history = history
        .iter()
        .map(|info| extract_partial_filenode(local_cache, repo_id, info))
        .collect();

    let history = PartialHistory { history };

    local_cache.fill(key, &history);
}

async fn enforce_remote_cache_timeout<T, Fut>(fut: Fut) -> Option<T>
where
    Fut: Future<Output = Option<T>>,
{
    match timeout(Duration::from_millis(REMOTE_CACHE_TIMEOUT_MILLIS), fut).await {
        Ok(r) => r,
        Err(_) => {
            STATS::remote_cache_timeouts.add_value(1);
            None
        }
    }
}

async fn enforce_sql_timeout<T, Fut>(fut: Fut) -> Result<T, ErrorKind>
where
    Fut: Future<Output = Result<T, Error>>,
{
    match timeout(Duration::from_millis(SQL_TIMEOUT_MILLIS), fut).await {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(ErrorKind::SqlError(e)),
        Err(_) => {
            STATS::sql_timeouts.add_value(1);
            Err(ErrorKind::SqlTimeout)
        }
    }
}

queries! {
    read SelectFilenode(
        repo_id: RepositoryId,
        path_hash: PathHashBytes,
        is_tree: i8,
        filenode: HgFileNodeId
    ) -> (
        HgFileNodeId,
        HgChangesetId,
        Option<HgFileNodeId>,
        Option<HgFileNodeId>,
        i8,
        Option<PathHashBytes>,
        Option<HgFileNodeId>,
    ) {
        "
        SELECT
            filenodes.filenode,
            filenodes.linknode,
            filenodes.p1,
            filenodes.p2,
            filenodes.has_copyinfo,
            fixedcopyinfo.frompath_hash,
            fixedcopyinfo.fromnode
        FROM filenodes
        LEFT JOIN fixedcopyinfo
           ON (
                   fixedcopyinfo.repo_id = filenodes.repo_id
               AND fixedcopyinfo.topath_hash = filenodes.path_hash
               AND fixedcopyinfo.tonode = filenodes.filenode
               AND fixedcopyinfo.is_tree = filenodes.is_tree
           )
        WHERE filenodes.repo_id = {repo_id}
          AND filenodes.path_hash = {path_hash}
          AND filenodes.is_tree = {is_tree}
          AND filenodes.filenode = {filenode}
        LIMIT 1
        "
    }


    read SelectAllFilenodes(
        repo_id: RepositoryId,
        path_hash: PathHashBytes,
        is_tree: i8,
    ) -> (
        HgFileNodeId,
        HgChangesetId,
        Option<HgFileNodeId>,
        Option<HgFileNodeId>,
        i8,
        Option<PathHashBytes>,
        Option<HgFileNodeId>,
    ) {
        "
        SELECT
            filenodes.filenode,
            filenodes.linknode,
            filenodes.p1,
            filenodes.p2,
            filenodes.has_copyinfo,
            fixedcopyinfo.frompath_hash,
            fixedcopyinfo.fromnode
        FROM filenodes
        LEFT JOIN fixedcopyinfo
           ON (
                   fixedcopyinfo.repo_id = filenodes.repo_id
               AND fixedcopyinfo.topath_hash = filenodes.path_hash
               AND fixedcopyinfo.tonode = filenodes.filenode
               AND fixedcopyinfo.is_tree = filenodes.is_tree
           )
        WHERE filenodes.repo_id = {repo_id}
          AND filenodes.path_hash = {path_hash}
          AND filenodes.is_tree = {is_tree}
        "
    }

    read SelectPaths(repo_id: RepositoryId, >list path_hashes: PathHashBytes) -> (PathHashBytes, PathBytes) {
        "SELECT path_hash, path
         FROM paths
         WHERE paths.repo_id = {repo_id}
           AND paths.path_hash in {path_hashes}"
    }
}
