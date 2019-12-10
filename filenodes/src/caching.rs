/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{Error, Result};
use cachelib::{get_cached_or_fill, VolatileLruCachePool};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::{future, Future, IntoFuture};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use memcache::{KeyGen, MemcacheClient, MEMCACHE_VALUE_MAX_SIZE};
use mercurial_types::{HgFileNodeId, RepoPath};
use mononoke_types::RepositoryId;
use rand::random;
use stats::{define_stats, Histogram, Timeseries};
use std::{collections::HashSet, convert::TryFrom, sync::Arc, time::Duration};

use super::thrift::{MC_CODEVER, MC_SITEVER};
use crate::{blake2_path_hash, thrift, FilenodeInfo, FilenodeInfoCached, Filenodes};

define_stats! {
    prefix = "mononoke.filenodes";
    gaf_compact_bytes: histogram(
        "get_all_filenodes.thrift_compact.bytes";
        500, 0, 1_000_000, AVG, SUM, COUNT; P 50; P 95; P 99
    ),
    point_filenode_hit: timeseries("point_filenode.memcache.hit"; RATE, SUM),
    point_filenode_miss: timeseries("point_filenode.memcache.miss"; RATE, SUM),
    point_filenode_internal_err: timeseries("point_filenode.memcache.internal_err"; RATE, SUM),
    point_filenode_deserialize_err: timeseries("point_filenode.memcache.deserialize_err"; RATE, SUM),
    point_filenode_pointers_err: timeseries("point_filenode.memcache.pointers_err"; RATE, SUM),
    gaf_hit: timeseries("get_all_filenodes.memcache.hit"; RATE, SUM),
    gaf_miss: timeseries("get_all_filenodes.memcache.miss"; RATE, SUM),
    gaf_pointers: timeseries("get_all_filenodes.memcache.pointers"; RATE, SUM),
    gaf_internal_err: timeseries("get_all_filenodes.memcache.internal_err"; RATE, SUM),
    gaf_deserialize_err: timeseries("get_all_filenodes.memcache.deserialize_err"; RATE, SUM),
    gaf_pointers_err: timeseries("get_all_filenodes.memcache.pointers_err"; RATE, SUM),
}

const TTL_SEC: u64 = 8 * 60 * 60;
// Adding a random to TTL helps preventing eviction of all related keys at once
const TTL_SEC_RAND: u64 = 30 * 60; // 30min

type Pointer = i64;
#[derive(Clone)]
struct PathHash(String);

pub struct CachingFilenodes {
    filenodes: Arc<dyn Filenodes>,
    cache_pool: VolatileLruCachePool,
    memcache: MemcacheClient,
    keygen: KeyGen,
}

impl CachingFilenodes {
    pub fn new(
        fb: FacebookInit,
        filenodes: Arc<dyn Filenodes>,
        cache_pool: VolatileLruCachePool,
        backing_store_name: impl ToString,
        backing_store_params: impl ToString,
    ) -> Self {
        let key_prefix = format!(
            "scm.mononoke.filenodes.{}.{}",
            backing_store_name.to_string(),
            backing_store_params.to_string(),
        );

        Self {
            filenodes,
            cache_pool,
            memcache: MemcacheClient::new(fb),
            keygen: KeyGen::new(key_prefix, MC_CODEVER as u32, MC_SITEVER as u32),
        }
    }
}

impl Filenodes for CachingFilenodes {
    fn add_filenodes(
        &self,
        ctx: CoreContext,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        self.filenodes.add_filenodes(ctx, info, repo_id)
    }

    fn add_or_replace_filenodes(
        &self,
        ctx: CoreContext,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        self.filenodes.add_or_replace_filenodes(ctx, info, repo_id)
    }

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        filenode_id: HgFileNodeId,
        repo_id: RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error> {
        let cache_key = format!("{}.{}.{}", repo_id.prefix(), filenode_id, path).to_string();
        cloned!(self.filenodes, self.memcache, self.keygen);

        get_cached_or_fill(&self.cache_pool, cache_key, || {
            let path_hash = PathHash({
                let path = match path.mpath() {
                    Some(path) => path.to_vec(),
                    None => Vec::new(),
                };
                blake2_path_hash(&path).to_string()
            });
            get_single_filenode_from_memcache(
                self.memcache.clone(),
                self.keygen.clone(),
                repo_id,
                filenode_id,
                &path_hash,
            )
            .then({
                cloned!(filenode_id, path, repo_id);
                move |res| match res {
                    Ok(filenode) => future::ok(Some(filenode.into())).left_future(),
                    Err(()) => filenodes
                        .get_filenode(ctx, &path, filenode_id, repo_id)
                        .inspect(move |maybefilenode| {
                            if let Some(filenode) = maybefilenode {
                                schedule_fill_single_filenode_memcache(
                                    filenode.clone(),
                                    memcache,
                                    keygen,
                                    repo_id,
                                    filenode_id,
                                    path_hash,
                                )
                            }
                        })
                        .map(|info| info.map(FilenodeInfoCached::from))
                        .right_future(),
                }
            })
            .boxify()
        })
        .and_then(|info| info.map(FilenodeInfo::try_from).transpose())
        .boxify()
    }

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        repo_id: RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        let path_hash = PathHash({
            let path = match path.mpath() {
                Some(path) => path.to_vec(),
                None => Vec::new(),
            };
            blake2_path_hash(&path).to_string()
        });

        cloned!(self.filenodes, self.memcache, self.keygen, path, repo_id);

        get_all_filenodes_from_memcache(
            memcache.clone(),
            keygen.clone(),
            repo_id.clone(),
            path_hash.clone(),
        )
        .then(move |from_memcache| {
            if let Ok(from_memcache) = from_memcache {
                return future::ok(from_memcache).left_future();
            }

            filenodes
                .get_all_filenodes_maybe_stale(ctx, &path, repo_id)
                .inspect(move |all_filenodes| {
                    schedule_fill_all_filenodes_memcache(
                        all_filenodes,
                        memcache,
                        keygen,
                        repo_id,
                        path_hash,
                    )
                })
                .right_future()
        })
        .boxify()
    }
}

// Local error type to help with proper logging metrics
enum ErrorKind {
    // error came from calling memcache API
    MemcacheInternal,
    // value returned from memcache was None
    Missing,
    // deserialization of memcache data to Rust structures via thrift failed
    Deserialization,
    // any problem in pointers logic - deserialization or missing data
    Pointers,
}

fn get_mc_key_for_single_filenode(
    keygen: &KeyGen,
    repo_id: RepositoryId,
    filenode: HgFileNodeId,
    path_hash: &PathHash,
) -> String {
    keygen.key(format!("{}.{}.{}", repo_id.id(), filenode, path_hash.0))
}

fn get_mc_key_for_filenodes_list(
    keygen: &KeyGen,
    repo_id: RepositoryId,
    path_hash: &PathHash,
) -> String {
    keygen.key(format!("{}.{}", repo_id.id(), path_hash.0))
}

fn get_mc_key_for_filenodes_list_pointer(
    keygen: &KeyGen,
    repo_id: RepositoryId,
    path_hash: &PathHash,
    pointer: Pointer,
) -> String {
    keygen.key(format!("{}.{}.{}", repo_id.id(), path_hash.0, pointer))
}

fn get_single_filenode_from_memcache(
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    filenode: HgFileNodeId,
    path_hash: &PathHash,
) -> impl Future<Item = FilenodeInfo, Error = ()> {
    memcache
        .get(get_mc_key_for_single_filenode(
            &keygen, repo_id, filenode, path_hash,
        ))
        .map_err(|()| ErrorKind::MemcacheInternal)
        .and_then(|maybe_serialized| maybe_serialized.ok_or(ErrorKind::Missing))
        .and_then(|serialized| {
            let thrift_entry: Result<thrift::FilenodeInfo, ErrorKind> =
                compact_protocol::deserialize(Vec::from(serialized))
                    .map_err(|_| ErrorKind::Deserialization);

            let thrift_entry = thrift_entry.and_then(|entry| {
                FilenodeInfo::from_thrift(entry).map_err(|_| ErrorKind::Deserialization)
            });
            thrift_entry
        })
        .then(move |res| {
            match res {
                Ok(res) => {
                    STATS::point_filenode_hit.add_value(1);
                    return Ok(res);
                }
                Err(ErrorKind::MemcacheInternal) => STATS::point_filenode_internal_err.add_value(1),
                Err(ErrorKind::Missing) => STATS::point_filenode_miss.add_value(1),
                Err(ErrorKind::Deserialization) => {
                    STATS::point_filenode_deserialize_err.add_value(1)
                }
                // Shouldn't happen for single filenode, but let's log just in case
                Err(ErrorKind::Pointers) => STATS::point_filenode_pointers_err.add_value(1),
            }
            Err(())
        })
}

fn get_all_filenodes_from_memcache(
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    path_hash: PathHash,
) -> impl Future<Item = Vec<FilenodeInfo>, Error = ()> {
    // helper function for deserializing list of thrift FilenodeInfo into rust structure with proper
    // error returned
    fn deserialize_list(list: Vec<thrift::FilenodeInfo>) -> Result<Vec<FilenodeInfo>, ErrorKind> {
        let res: Result<Vec<_>> = list.into_iter().map(FilenodeInfo::from_thrift).collect();
        res.map_err(|_| ErrorKind::Deserialization)
    }

    memcache
        .get(get_mc_key_for_filenodes_list(&keygen, repo_id, &path_hash))
        .map_err(|()| ErrorKind::MemcacheInternal)
        .and_then(|maybe_serialized| maybe_serialized.ok_or(ErrorKind::Missing))
        .and_then(|serialized| {
            compact_protocol::deserialize(Vec::from(serialized))
                .map_err(|_| ErrorKind::Deserialization)
        })
        .and_then(move |deserialized| match deserialized {
            thrift::FilenodeInfoList::UnknownField(_) => {
                Err(ErrorKind::Deserialization).into_future().left_future()
            }
            thrift::FilenodeInfoList::Data(list) => {
                deserialize_list(list).into_future().left_future()
            }
            thrift::FilenodeInfoList::Pointers(list) => {
                STATS::gaf_pointers.add_value(1);

                let read_chunks_fut = list.into_iter().map(move |pointer| {
                    memcache
                        .get(get_mc_key_for_filenodes_list_pointer(
                            &keygen, repo_id, &path_hash, pointer,
                        ))
                        .then(|res| match res {
                            Ok(Some(res)) => Ok(res),
                            Ok(None) | Err(_) => Err(ErrorKind::Pointers),
                        })
                });

                future::join_all(read_chunks_fut)
                    .and_then(|chunks| {
                        let serialized: Vec<_> = chunks.into_iter().flat_map(Vec::from).collect();
                        compact_protocol::deserialize(serialized).map_err(|_| ErrorKind::Pointers)
                    })
                    .and_then(|deserialized| match deserialized {
                        thrift::FilenodeInfoList::Data(list) => {
                            deserialize_list(list).into_future().left_future()
                        }
                        _ => future::err(ErrorKind::Pointers).right_future(),
                    })
                    .right_future()
            }
        })
        .then(move |res| {
            match res {
                Ok(res) => {
                    STATS::gaf_hit.add_value(1);
                    return Ok(res);
                }
                Err(ErrorKind::MemcacheInternal) => STATS::gaf_internal_err.add_value(1),
                Err(ErrorKind::Missing) => STATS::gaf_miss.add_value(1),
                Err(ErrorKind::Deserialization) => STATS::gaf_deserialize_err.add_value(1),
                Err(ErrorKind::Pointers) => STATS::gaf_pointers_err.add_value(1),
            }
            Err(())
        })
}

fn schedule_fill_single_filenode_memcache(
    filenode: FilenodeInfo,
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    filenode_id: HgFileNodeId,
    path_hash: PathHash,
) {
    let serialized = compact_protocol::serialize(&filenode.into_thrift());

    // Quite unlikely that single filenode will be bigger than MEMCACHE_VALUE_MAX_SIZE
    // It's probably not even worth logging it
    if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        tokio::spawn(memcache.set(
            get_mc_key_for_single_filenode(&keygen, repo_id, filenode_id, &path_hash),
            serialized,
        ));
    }
}

fn schedule_fill_all_filenodes_memcache(
    all_filenodes: &Vec<FilenodeInfo>,
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    path_hash: PathHash,
) {
    let serialized = {
        let all_filenodes = thrift::FilenodeInfoList::Data(
            all_filenodes
                .into_iter()
                .map(|filenode_info| filenode_info.clone().into_thrift())
                .collect(),
        );
        compact_protocol::serialize(&all_filenodes)
    };

    STATS::gaf_compact_bytes.add_value(serialized.len() as i64);

    let serialized_filenode_info_list_fut = if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        future::ok(serialized).left_future()
    } else {
        let write_chunks_fut = serialized
            .chunks(MEMCACHE_VALUE_MAX_SIZE)
            .map(Vec::from) // takes ownership
            .zip(PointersIter::new())
            .map({
                cloned!(memcache, keygen, repo_id, path_hash);
                move |(chunk, pointer)| {
                    memcache
                        .set_with_ttl(
                            get_mc_key_for_filenodes_list_pointer(
                                &keygen,
                                repo_id,
                                &path_hash,
                                pointer,
                            ),
                            chunk,
                            // give chunks non-random max TTL_SEC_RAND so that they always live
                            // longer than the pointer
                            Duration::from_secs(TTL_SEC + TTL_SEC_RAND),
                        )
                        .map(move |_| pointer)
                }
            })
            .collect::<Vec<_>>();

        future::join_all(write_chunks_fut)
            .map(move |pointers| {
                compact_protocol::serialize(&thrift::FilenodeInfoList::Pointers(pointers))
            })
            .right_future()
    };

    tokio::spawn(
        serialized_filenode_info_list_fut.and_then(move |serialized| {
            memcache.set_with_ttl(
                get_mc_key_for_filenodes_list(&keygen, repo_id, &path_hash),
                serialized,
                Duration::from_secs(TTL_SEC + random::<u64>() % TTL_SEC_RAND),
            )
        }),
    );
}

/// Infinite iterator over unique and random i64 values
struct PointersIter {
    seen: HashSet<Pointer>,
}

impl PointersIter {
    fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }
}

impl Iterator for PointersIter {
    type Item = Pointer;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let pointer = random();
            if self.seen.insert(pointer) {
                break Some(pointer);
            }
        }
    }
}
