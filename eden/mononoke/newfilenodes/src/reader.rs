/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use context::PerfCounterType;
use faster_hex::hex_encode;
use futures::future;
use futures::future::Future;
use itertools::Itertools;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use path_hash::PathBytes;
use path_hash::PathHashBytes;
use path_hash::PathWithHash;
use rand::thread_rng;
use rand::Rng;
use sql::queries;
use sql::Connection;
use stats::prelude::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error as DeriveError;
use tokio::time::timeout;
use tunables::tunables;

use filenodes::FilenodeInfo;
use filenodes::FilenodeRangeResult;
use filenodes::FilenodeResult;
use filenodes::PreparedFilenode;

use crate::connections::AcquireReason;
use crate::connections::Connections;
use crate::local_cache::CacheKey;
use crate::local_cache::LocalCache;
use crate::remote_cache::RemoteCache;
use crate::shards::Shards;
use crate::sql_timeout_knobs;
use crate::structs::CachedFilenode;
use crate::structs::CachedHistory;

define_stats! {
    prefix = "mononoke.filenodes";
    gets: timeseries(Sum),
    gets_master: timeseries(Sum),
    gets_disabled: timeseries(Sum),
    range_gets: timeseries(Sum),
    range_gets_disabled: timeseries(Sum),
    path_gets: timeseries(Sum),
    get_local_cache_misses: timeseries(Sum),
    range_local_cache_misses: timeseries(Sum),
    remote_cache_timeouts: timeseries(Sum),
    sql_timeouts: timeseries(Sum),
    too_big_history: timeseries(Sum),
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

    #[error("Internal error: SQL error")]
    SqlError(#[source] Error),

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
    let is_tree = pwh.is_tree as u8;
    hex_encode(pwh.hash.0.as_ref(), &mut v).expect("failed to hex encode");
    let key = format!(
        "filenode.{}.{}.{}.{}",
        repo_id.id(),
        filenode,
        unsafe { String::from_utf8_unchecked(v) },
        is_tree
    );

    CacheKey {
        key,
        value: PhantomData,
    }
}

pub fn history_cache_key(
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    limit: Option<u64>,
) -> CacheKey<Option<CachedHistory>> {
    let mut v = vec![0; pwh.hash.0.len() * 2];
    let is_tree = pwh.is_tree as u8;
    hex_encode(pwh.hash.0.as_ref(), &mut v).expect("failed to hex encode");
    let key = match limit {
        Some(limit) => format!(
            "history.{}.limit.{}.{}.{}",
            repo_id.id(),
            limit,
            unsafe { String::from_utf8_unchecked(v) },
            is_tree
        ),
        None => format!(
            "history.{}.{}.{}",
            repo_id.id(),
            unsafe { String::from_utf8_unchecked(v) },
            is_tree
        ),
    };

    CacheKey {
        key,
        value: PhantomData,
    }
}
pub struct FilenodesReader {
    read_connections: Connections,
    read_master_connections: Connections,
    shards: Arc<Shards>,
    pub local_cache: LocalCache,
    pub remote_cache: RemoteCache,
}

impl FilenodesReader {
    pub fn new(
        read_connections: Vec<Connection>,
        read_master_connections: Vec<Connection>,
    ) -> Self {
        Self {
            shards: Arc::new(Shards::new(1000, 1000)),
            read_connections: Connections::new(read_connections),
            read_master_connections: Connections::new(read_master_connections),
            local_cache: LocalCache::Noop,
            remote_cache: RemoteCache::Noop,
        }
    }

    pub async fn get_filenode(
        self: Arc<Self>,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        path: &RepoPath,
        filenode: HgFileNodeId,
    ) -> Result<FilenodeResult<Option<FilenodeInfo>>, Error> {
        STATS::gets.add_value(1);

        let pwh = PathWithHash::from_repo_path_cow(Cow::Owned(path.clone()));
        let key = filenode_cache_key(repo_id, &pwh, &filenode);

        if let Some(cached) = self.local_cache.get(&key) {
            return Ok(FilenodeResult::Present(Some(cached.try_into()?)));
        }

        let ctx = ctx.clone();
        self.shards
            .clone()
            .with_filenodes(path, filenode, move || {
                async move {
                    // Now that we acquired the permit, check our cache again, in case the previous permit
                    // owner just filed the cache with the filenode we're looking for.
                    if let Some(cached) = self.local_cache.get(&key) {
                        return Ok(FilenodeResult::Present(Some(cached.try_into()?)));
                    }

                    STATS::get_local_cache_misses.add_value(1);

                    if let Some(info) =
                        enforce_remote_cache_timeout(self.remote_cache.get_filenode(&key)).await
                    {
                        self.local_cache.fill(&key, &(&info).into());
                        return Ok(FilenodeResult::Present(Some(info)));
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
                        Ok(FilenodeResult::Disabled) => {
                            return Ok(FilenodeResult::Disabled);
                        }
                        Ok(FilenodeResult::Present(Some(res))) => {
                            return Ok(FilenodeResult::Present(Some(res.try_into()?)));
                        }
                        Ok(FilenodeResult::Present(None))
                        | Err(ErrorKind::FixedCopyInfoMissing(_))
                        | Err(ErrorKind::PathNotFound(_)) => {
                            // If the filenode wasn't found, or its copy info was missing, it might be present
                            // on the master.
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }

                    let ratio = tunables().get_filenodes_master_fallback_ratio();
                    if ratio > 0 {
                        let mut rng = thread_rng();
                        let n = rng.gen_range(0..ratio);
                        if n > 0 {
                            return Ok(FilenodeResult::Disabled);
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

                    match res {
                        FilenodeResult::Present(res) => {
                            let res = res.map(|res| res.try_into()).transpose()?;
                            Ok(FilenodeResult::Present(res))
                        }
                        FilenodeResult::Disabled => Ok(FilenodeResult::Disabled),
                    }
                }
            })
            .await?
    }

    pub async fn get_all_filenodes_for_path(
        self: Arc<Self>,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        path: &RepoPath,
        limit: Option<u64>,
    ) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>, Error> {
        STATS::range_gets.add_value(1);

        let pwh = PathWithHash::from_repo_path_cow(Cow::Owned(path.clone()));
        let key = history_cache_key(repo_id, &pwh, limit);

        if let Some(cached) = self.local_cache.get(&key) {
            return convert_cached_filenodes(cached);
        }
        let ctx = ctx.clone();
        self.shards
            .clone()
            .with_history(path, move || {
                async move {
                    // See above for rationale here.
                    if let Some(cached) = self.local_cache.get(&key) {
                        return convert_cached_filenodes(cached);
                    }

                    STATS::range_local_cache_misses.add_value(1);

                    if let Some(info) =
                        enforce_remote_cache_timeout(self.remote_cache.get_history(&key)).await
                    {
                        let info = info.into_option();
                        // TODO: We should compress if this is too big.
                        self.local_cache
                            .fill(&key, &info.as_ref().map(|info| info.into()));
                        match info {
                            Some(info) => {
                                return Ok(FilenodeRangeResult::Present(info));
                            }
                            None => {
                                return Ok(FilenodeRangeResult::TooBig);
                            }
                        }
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
                        limit,
                    )
                    .await?;

                    match res {
                        FilenodeRangeResult::Present(res) => {
                            Ok(FilenodeRangeResult::Present(res.try_into()?))
                        }
                        FilenodeRangeResult::TooBig => Ok(FilenodeRangeResult::TooBig),
                        FilenodeRangeResult::Disabled => Ok(FilenodeRangeResult::Disabled),
                    }
                }
            })
            .await?
    }

    pub fn prime_cache(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        filenodes: &[PreparedFilenode],
    ) {
        for c in filenodes {
            let pwh = PathWithHash::from_repo_path(&c.path);
            let key = filenode_cache_key(repo_id, &pwh, &c.info.filenode);
            self.local_cache.fill(&key, &(&c.info).into())
        }
    }
}

fn convert_cached_filenodes(
    cached: Option<CachedHistory>,
) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>, Error> {
    match cached {
        Some(cached) => Ok(FilenodeRangeResult::Present(cached.try_into()?)),
        None => Ok(FilenodeRangeResult::TooBig),
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
        self.local_cache.fill(self.key, &filenode);
        if let Ok(filenode) = filenode.try_into() {
            self.remote_cache.fill_filenode(self.key, filenode);
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
) -> Result<FilenodeResult<Option<CachedFilenode>>, ErrorKind> {
    if tunables().get_filenodes_disabled() {
        STATS::gets_disabled.add_value(1);
        return Ok(FilenodeResult::Disabled);
    }

    let partial = select_partial_filenode(connections, repo_id, pwh, filenode, recorder).await?;

    let partial = match partial {
        Some(partial) => partial,
        None => {
            return Ok(FilenodeResult::Present(None));
        }
    };

    let ret = match fill_paths(connections, pwh, repo_id, vec![partial], recorder)
        .await?
        .into_iter()
        .next()
    {
        Some(ret) => ret,
        None => {
            return Ok(FilenodeResult::Present(None));
        }
    };

    filler.fill(ret.clone());

    Ok(FilenodeResult::Present(Some(ret)))
}

async fn select_partial_filenode(
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    filenode: HgFileNodeId,
    recorder: &PerfCounterRecorder<'_>,
) -> Result<Option<PartialFilenode>, ErrorKind> {
    let connection = connections.checkout(pwh, AcquireReason::Filenodes);

    recorder.increment();

    let rows = enforce_sql_timeout(SelectFilenode::query(
        connection,
        &repo_id,
        &pwh.hash,
        pwh.sql_is_tree(),
        &filenode,
    ))
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
    key: &'a CacheKey<Option<CachedHistory>>,
}

impl<'a> HistoryCacheFiller<'a> {
    fn fill(&self, maybe_history: Option<CachedHistory>) {
        self.local_cache.fill(self.key, &maybe_history);
        let maybe_history = maybe_history.map(|history| history.try_into()).transpose();
        if let Ok(maybe_history) = maybe_history {
            self.remote_cache.fill_history(self.key, maybe_history);
        }
    }
}

async fn select_history_from_sql(
    filler: &HistoryCacheFiller<'_>,
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    recorder: &PerfCounterRecorder<'_>,
    limit: Option<u64>,
) -> Result<FilenodeRangeResult<CachedHistory>, Error> {
    if tunables().get_filenodes_disabled() {
        STATS::range_gets_disabled.add_value(1);
        return Ok(FilenodeRangeResult::Disabled);
    }

    let maybe_partial = select_partial_history(connections, repo_id, pwh, recorder, limit).await?;
    if let Some(partial) = maybe_partial {
        let history = fill_paths(connections, pwh, repo_id, partial, recorder).await?;
        let history = CachedHistory { history };
        filler.fill(Some(history.clone()));
        Ok(FilenodeRangeResult::Present(history))
    } else {
        filler.fill(None);
        Ok(FilenodeRangeResult::TooBig)
    }
}

async fn select_partial_history(
    connections: &Connections,
    repo_id: RepositoryId,
    pwh: &PathWithHash<'_>,
    recorder: &PerfCounterRecorder<'_>,
    limit: Option<u64>,
) -> Result<Option<Vec<PartialFilenode>>, ErrorKind> {
    let connection = connections.checkout(pwh, AcquireReason::History);

    recorder.increment();

    // Try to fetch one entry more - if we fetched limit + 1, then file
    // history is too big.
    let limit = limit.map(|l| l + 1);
    let rows = match limit {
        Some(limit) => {
            let rows = enforce_sql_timeout(SelectLimitedFilenodes::query(
                connection,
                &repo_id,
                &pwh.hash,
                pwh.sql_is_tree(),
                &limit,
            ))
            .await?;
            if rows.len() >= limit as usize {
                STATS::too_big_history.add_value(1);
                return Ok(None);
            }
            rows
        }
        None => {
            enforce_sql_timeout(SelectAllFilenodes::query(
                connection,
                &repo_id,
                &pwh.hash,
                pwh.sql_is_tree(),
            ))
            .await?
        }
    };

    let history = rows
        .into_iter()
        .map(convert_row_to_partial_filenode)
        .collect::<Result<Vec<PartialFilenode>, ErrorKind>>()?;

    // TODO: It'd be nice to have some eviction here.
    // TODO: It'd be nice to chain those.

    Ok(Some(history))
}

fn convert_row_to_partial_filenode(row: FilenodeRow) -> Result<PartialFilenode, ErrorKind> {
    let (filenode, linknode, p1, p2, has_copyinfo, from_path_hash, from_node) = row;

    let copyfrom = if has_copyinfo == 0 {
        None
    } else {
        let from_path_hash = from_path_hash.ok_or(ErrorKind::FixedCopyInfoMissing(filenode))?;

        let from_node = from_node.ok_or(ErrorKind::FixedCopyInfoMissing(filenode))?;

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
        .group_by(|path_hash| connections.shard_id(path_hash))
        .into_iter()
        .map(|(shard_id, group)| {
            let group = group.collect::<Vec<_>>();

            STATS::path_gets.add_value(group.len() as i64);

            async move {
                recorder.increment();

                let connection = connections.checkout_by_shard_id(shard_id, AcquireReason::Paths);

                let output =
                    enforce_sql_timeout(SelectPaths::query(connection, &repo_id, &group[..]))
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
    if !sql_timeout_knobs::should_enforce_sql_timeouts() {
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

    read SelectLimitedFilenodes(
        repo_id: RepositoryId,
        path_hash: PathHashBytes,
        is_tree: i8,
        limit: u64,
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
        LIMIT {limit}
        "
    }

    read SelectPaths(repo_id: RepositoryId, >list path_hashes: PathHashBytes) -> (PathHashBytes, PathBytes) {
        "SELECT path_hash, path
         FROM paths
         WHERE paths.repo_id = {repo_id}
           AND paths.path_hash in {path_hashes}"
    }
}
