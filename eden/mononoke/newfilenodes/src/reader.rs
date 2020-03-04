/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::{CoreContext, PerfCounterType};
use faster_hex::hex_encode;
use futures::{
    compat::Future01CompatExt,
    future::{self, Future},
};
use itertools::Itertools;
use mercurial_types::{HgChangesetId, HgFileNodeId};
use mononoke_types::{RepoPath, RepositoryId};
use scopeguard;
use sql::{queries, Connection};
use stats::prelude::*;
use std::collections::HashMap;
use std::convert::TryInto;
use std::marker::PhantomData;
use std::time::Duration;
use thiserror::Error as DeriveError;
use tokio_preview::time::timeout;

use filenodes::FilenodeInfo;

use crate::connections::{AcquireReason, Connections};
use crate::local_cache::{CacheKey, LocalCache};
use crate::remote_cache::RemoteCache;
use crate::shards::Shards;
use crate::structs::{CachedFilenode, CachedHistory, PathBytes, PathHashBytes, PathWithHash};
use crate::tunables;

define_stats! {
    prefix = "mononoke.filenodes";
    gets: timeseries(Sum),
    gets_master: timeseries(Sum),
    range_gets: timeseries(Sum),
    path_gets: timeseries(Sum),
    get_local_cache_misses: timeseries(Sum),
    range_local_cache_misses: timeseries(Sum),
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
}

struct PerfCounterRecorder<'a> {
    ctx: &'a CoreContext,
    counter: PerfCounterType,
}

impl<'a> PerfCounterRecorder<'a> {
    fn increment(&self) {
        self.ctx.perf_counters().increment_counter(self.counter);
    }
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

pub fn filenode_cache_key(
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: &HgFileNodeId,
) -> CacheKey<CachedFilenode> {
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

pub fn history_cache_key(repo_id: RepositoryId, pwh: &PathWithHash<'_>) -> CacheKey<CachedHistory> {
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

pub struct FilenodesReader {
    read_connections: Connections,
    read_master_connections: Connections,
    shards: Shards,
    pub local_cache: LocalCache,
    pub remote_cache: RemoteCache,
}

impl FilenodesReader {
    pub fn new(
        read_connections: Vec<Connection>,
        read_master_connections: Vec<Connection>,
    ) -> Self {
        Self {
            shards: Shards::new(1000, 1000),
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
        path: &RepoPath,
        filenode: HgFileNodeId,
    ) -> Result<Option<FilenodeInfo>, Error> {
        STATS::gets.add_value(1);

        let pwh = PathWithHash::from_repo_path(&path);
        let key = filenode_cache_key(repo_id, &pwh, &filenode);

        if let Some(cached) = self.local_cache.get(&key) {
            return Ok(Some(cached.try_into()?));
        }

        let permit = self.shards.acquire_filenodes(&path, filenode).await;
        scopeguard::defer! { drop(permit); };

        // Now that we acquired the permit, check our cache again, in case the previous permit
        // owner just filed the cache with the filenode we're looking for.
        if let Some(cached) = self.local_cache.get(&key) {
            return Ok(Some(cached.try_into()?));
        }

        STATS::get_local_cache_misses.add_value(1);

        if let Some(info) = enforce_remote_cache_timeout(self.remote_cache.get_filenode(&key)).await
        {
            self.local_cache.fill(&key, &(&info).into());
            return Ok(Some(info));
        }

        let cache_filler = FilenodeCacheFiller {
            local_cache: &self.local_cache,
            remote_cache: &self.remote_cache,
            key: &key,
        };

        match select_filenode_from_sql(
            cache_filler,
            &self.read_connections,
            repo_id,
            &pwh,
            filenode,
            &PerfCounterRecorder {
                ctx: &ctx,
                counter: PerfCounterType::SqlReadsReplica,
            },
        )
        .await
        {
            Ok(Some(res)) => {
                return Ok(Some(res.try_into()?));
            }
            Ok(None)
            | Err(ErrorKind::FixedCopyInfoMissing(_))
            | Err(ErrorKind::PathNotFound(_)) => {
                // If the filenode wasn't found, or its copy info was missing, it might be present
                // on the master.
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        STATS::gets_master.add_value(1);

        let res = select_filenode_from_sql(
            cache_filler,
            &self.read_master_connections,
            repo_id,
            &pwh,
            filenode,
            &PerfCounterRecorder {
                ctx: &ctx,
                counter: PerfCounterType::SqlReadsMaster,
            },
        )
        .await?;

        return res.map(|r| r.try_into()).transpose();
    }

    pub async fn get_all_filenodes_for_path(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        path: &RepoPath,
    ) -> Result<Vec<FilenodeInfo>, Error> {
        STATS::range_gets.add_value(1);

        let pwh = PathWithHash::from_repo_path(&path);
        let key = history_cache_key(repo_id, &pwh);

        if let Some(cached) = self.local_cache.get(&key) {
            return Ok(cached.try_into()?);
        }

        let permit = self.shards.acquire_history(&path).await;
        scopeguard::defer! { drop(permit); };

        // See above for rationale here.
        if let Some(cached) = self.local_cache.get(&key) {
            return Ok(cached.try_into()?);
        }

        STATS::range_local_cache_misses.add_value(1);

        if let Some(info) = enforce_remote_cache_timeout(self.remote_cache.get_history(&key)).await
        {
            // TODO: We should compress if this is too big.
            self.local_cache.fill(&key, &(&info).into());
            return Ok(info);
        }

        let cache_filler = HistoryCacheFiller {
            local_cache: &self.local_cache,
            remote_cache: &self.remote_cache,
            key: &key,
        };

        let res = select_history_from_sql(
            &cache_filler,
            &self.read_connections,
            repo_id,
            &pwh,
            &PerfCounterRecorder {
                ctx: &ctx,
                counter: PerfCounterType::SqlReadsReplica,
            },
        )
        .await?;

        Ok(res.try_into()?)
    }
}

#[derive(Copy, Clone)]
struct FilenodeCacheFiller<'a> {
    local_cache: &'a LocalCache,
    remote_cache: &'a RemoteCache,
    key: &'a CacheKey<CachedFilenode>,
}

impl<'a> FilenodeCacheFiller<'a> {
    fn fill(&self, filenode: CachedFilenode) {
        self.local_cache.fill(&self.key, &filenode);
        if let Ok(filenode) = filenode.try_into() {
            self.remote_cache.fill_filenode(&self.key, filenode);
        }
    }
}

struct PartialFilenode {
    filenode: HgFileNodeId,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    copyfrom: Option<(PathHashBytes, HgFileNodeId)>,
    linknode: HgChangesetId,
}

async fn select_filenode_from_sql(
    filler: FilenodeCacheFiller<'_>,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: HgFileNodeId,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<Option<CachedFilenode>, ErrorKind> {
    let partial = select_partial_filenode(connections, repo_id, pwh, filenode, recorder).await?;

    let partial = match partial {
        Some(partial) => partial,
        None => {
            return Ok(None);
        }
    };

    let ret = match fill_paths(connections, pwh, repo_id, vec![partial], recorder)
        .await?
        .into_iter()
        .next()
    {
        Some(ret) => ret,
        None => {
            return Ok(None);
        }
    };

    filler.fill(ret.clone());

    Ok(Some(ret))
}

async fn select_partial_filenode(
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: HgFileNodeId,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<Option<PartialFilenode>, ErrorKind> {
    let connection = connections.checkout(&pwh, AcquireReason::Filenodes);

    recorder.increment();

    let rows = enforce_sql_timeout(
        SelectFilenode::query(
            &connection,
            &repo_id,
            &pwh.hash,
            pwh.sql_is_tree(),
            &filenode,
        )
        .compat(),
    )
    .await?;

    match rows.into_iter().next() {
        Some(row) => {
            let partial = convert_row_to_partial_filenode(row)?;
            Ok(Some(partial))
        }
        None => Ok(None),
    }
}

#[derive(Copy, Clone)]
struct HistoryCacheFiller<'a> {
    local_cache: &'a LocalCache,
    remote_cache: &'a RemoteCache,
    key: &'a CacheKey<CachedHistory>,
}

impl<'a> HistoryCacheFiller<'a> {
    fn fill(&self, history: CachedHistory) {
        self.local_cache.fill(&self.key, &history);
        if let Ok(history) = history.try_into() {
            self.remote_cache.fill_history(&self.key, history);
        }
    }
}

async fn select_history_from_sql(
    filler: &HistoryCacheFiller<'_>,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<CachedHistory, Error> {
    let partial = select_partial_history(&connections, repo_id, &pwh, recorder).await?;
    let history = fill_paths(&connections, &pwh, repo_id, partial, recorder).await?;
    let history = CachedHistory { history };
    filler.fill(history.clone());
    Ok(history)
}

async fn select_partial_history(
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<Vec<PartialFilenode>, ErrorKind> {
    let connection = connections.checkout(&pwh, AcquireReason::History);

    recorder.increment();

    let rows = enforce_sql_timeout(
        SelectAllFilenodes::query(&connection, &repo_id, &pwh.hash, pwh.sql_is_tree()).compat(),
    )
    .await?;

    let history = rows
        .into_iter()
        .map(|row| convert_row_to_partial_filenode(row))
        .collect::<Result<Vec<PartialFilenode>, ErrorKind>>()?;

    // TODO: It'd be nice to have some eviction here.
    // TODO: It'd be nice to chain those.

    Ok(history)
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
    pwh: &PathWithHash<'_>,
    repo_id: RepositoryId,
    rows: Vec<PartialFilenode>,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<Vec<CachedFilenode>, ErrorKind> {
    let path_hashes_to_fetch = rows
        .iter()
        .filter_map(|r| r.copyfrom.as_ref().map(|c| c.0.clone()));

    let path_hashes_to_paths =
        select_paths(connections, repo_id, path_hashes_to_fetch, recorder).await?;

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
                    Some((pwh.is_tree, from_path, from_node))
                }
                None => None,
            };

            let ret = CachedFilenode {
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
    repo_id: RepositoryId,
    iter: I,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<HashMap<PathHashBytes, PathBytes>, ErrorKind> {
    let futs = iter
        .group_by(|path_hash| connections.shard_id(&path_hash))
        .into_iter()
        .map(|(shard_id, group)| {
            let group = group.collect::<Vec<_>>();

            STATS::path_gets.add_value(group.len() as i64);

            async move {
                recorder.increment();

                let connection = connections.checkout_by_shard_id(shard_id, AcquireReason::Paths);

                let output = enforce_sql_timeout(
                    SelectPaths::query(&connection, &repo_id, &group[..]).compat(),
                )
                .await?
                .into_iter()
                .collect::<HashMap<_, _>>();

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
    if !tunables::should_enforce_sql_timeouts() {
        return fut.await.map_err(ErrorKind::SqlError);
    }

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
