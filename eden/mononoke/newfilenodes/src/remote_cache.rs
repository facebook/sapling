/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use caching_ext::MemcacheHandler;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::future::try_join_all;
use memcache::KeyGen;
use memcache::MemcacheClient;
use memcache::MEMCACHE_VALUE_MAX_SIZE;
use rand::random;
use stats::prelude::*;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;
use time_ext::DurationExt;

use crate::local_cache::CacheKey;
use crate::structs::CachedFilenode;
use crate::structs::CachedHistory;

use filenodes::thrift;
use filenodes::thrift::MC_CODEVER;
use filenodes::thrift::MC_SITEVER;
use filenodes::FilenodeInfo;

define_stats! {
    prefix = "mononoke.filenodes";
    gaf_compact_bytes: histogram(
        "get_all_filenodes.thrift_compact.bytes";
        500, 0, 1_000_000, Average, Sum, Count; P 50; P 95; P 99
    ),
    point_filenode_hit: timeseries("point_filenode.memcache.hit"; Sum),
    point_filenode_miss: timeseries("point_filenode.memcache.miss"; Sum),
    point_filenode_internal_err: timeseries("point_filenode.memcache.internal_err"; Sum),
    point_filenode_deserialize_err: timeseries("point_filenode.memcache.deserialize_err"; Sum),
    point_filenode_pointers_err: timeseries("point_filenode.memcache.pointers_err"; Sum),
    gaf_hit: timeseries("get_all_filenodes.memcache.hit"; Sum),
    gaf_miss: timeseries("get_all_filenodes.memcache.miss"; Sum),
    gaf_pointers: timeseries("get_all_filenodes.memcache.pointers"; Sum),
    gaf_internal_err: timeseries("get_all_filenodes.memcache.internal_err"; Sum),
    gaf_deserialize_err: timeseries("get_all_filenodes.memcache.deserialize_err"; Sum),
    gaf_pointers_err: timeseries("get_all_filenodes.memcache.pointers_err"; Sum),
    get_latency: histogram("get.memcache.duration_us"; 100, 0, 10000, Average, Count; P 50; P 95; P 100),
    get_history: histogram("get_history.memcache.duration_us"; 100, 0, 10000, Average, Count; P 50; P 95; P 100),
}

const SITEVER_OVERRIDE_VAR: &str = "MONONOKE_OVERRIDE_FILENODES_MC_SITEVER";

const TTL_SEC: u64 = 8 * 60 * 60;

// Adding a random to TTL helps preventing eviction of all related keys at once
const TTL_SEC_RAND: u64 = 30 * 60; // 30min

pub enum RemoteCache {
    Memcache(MemcacheCache),
    Noop,
}

impl RemoteCache {
    // TODO: Can we optimize to reuse the existing PathWithHash we got?
    pub async fn get_filenode(&self, key: &CacheKey<CachedFilenode>) -> Option<FilenodeInfo> {
        match self {
            Self::Memcache(memcache) => {
                let now = Instant::now();

                let ret =
                    get_single_filenode_from_memcache(&memcache.memcache, &memcache.keygen, key)
                        .await;

                let elapsed = now.elapsed().as_micros_unchecked() as i64;
                STATS::get_latency.add_value(elapsed);

                ret
            }
            Self::Noop => None,
        }
    }

    // TODO: Need to use the same CacheKey here.
    pub fn fill_filenode(&self, key: &CacheKey<CachedFilenode>, filenode: FilenodeInfo) {
        match self {
            Self::Memcache(memcache) => {
                schedule_fill_filenode(&memcache.memcache, &memcache.keygen, key, filenode)
            }
            Self::Noop => {}
        }
    }

    pub async fn get_history(
        &self,
        key: &CacheKey<Option<CachedHistory>>,
    ) -> Option<RemoteCachedHistory> {
        match self {
            Self::Memcache(memcache) => {
                let now = Instant::now();

                let ret =
                    get_history_from_memcache(&memcache.memcache, &memcache.keygen, key).await;

                let elapsed = now.elapsed().as_micros_unchecked() as i64;
                STATS::get_history.add_value(elapsed);

                ret
            }
            Self::Noop => None,
        }
    }

    // TODO: Take ownership of key
    pub fn fill_history(
        &self,
        key: &CacheKey<Option<CachedHistory>>,
        filenodes: Option<Vec<FilenodeInfo>>,
    ) {
        match self {
            Self::Memcache(memcache) => schedule_fill_history(
                memcache.memcache.clone(),
                memcache.keygen.clone(),
                key.clone(),
                filenodes,
            ),
            Self::Noop => {}
        }
    }
}

#[derive(Eq, PartialEq)]
pub enum RemoteCachedHistory {
    History(Vec<FilenodeInfo>),
    TooBig,
}

impl RemoteCachedHistory {
    pub fn into_option(self) -> Option<Vec<FilenodeInfo>> {
        use RemoteCachedHistory::*;
        match self {
            History(history) => Some(history),
            TooBig => None,
        }
    }
}

type Pointer = i64;

#[derive(Clone)]
pub struct MemcacheCache {
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl MemcacheCache {
    pub fn new(fb: FacebookInit, backing_store_name: &str, backing_store_params: &str) -> Self {
        let key_prefix = format!(
            "scm.mononoke.filenodes.{}.{}",
            backing_store_name, backing_store_params,
        );

        let mc_sitever = match std::env::var(&SITEVER_OVERRIDE_VAR) {
            Ok(v) => v.parse().unwrap_or(MC_SITEVER as u32),
            Err(_) => MC_SITEVER as u32,
        };

        Self {
            memcache: MemcacheHandler::from(
                MemcacheClient::new(fb).expect("Memcache initialization failed"),
            ),
            keygen: KeyGen::new(key_prefix, MC_CODEVER as u32, mc_sitever),
        }
    }
}

fn get_mc_key_for_filenodes_list_chunk(
    keygen: &KeyGen,
    key: &CacheKey<Option<CachedHistory>>,
    pointer: Pointer,
) -> String {
    keygen.key(format!("{}.{}", key.key, pointer))
}

async fn get_single_filenode_from_memcache(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<CachedFilenode>,
) -> Option<FilenodeInfo> {
    let key = keygen.key(&key.key);

    let serialized = match memcache.get(key).await {
        Ok(Some(serialized)) => serialized,
        Ok(None) => {
            STATS::point_filenode_miss.add_value(1);
            return None;
        }
        Err(_) => {
            STATS::point_filenode_internal_err.add_value(1);
            return None;
        }
    };

    let thrift = match compact_protocol::deserialize(&serialized) {
        Ok(thrift) => thrift,
        Err(_) => {
            STATS::point_filenode_deserialize_err.add_value(1);
            return None;
        }
    };

    let info = match FilenodeInfo::from_thrift(thrift) {
        Ok(info) => info,
        Err(_) => {
            STATS::point_filenode_deserialize_err.add_value(1);
            return None;
        }
    };

    STATS::point_filenode_hit.add_value(1);

    Some(info)
}

async fn get_history_from_memcache(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<Option<CachedHistory>>,
) -> Option<RemoteCachedHistory> {
    // helper function for deserializing list of thrift FilenodeInfo into rust structure with proper
    // error returned
    fn deserialize_list(list: Vec<thrift::FilenodeInfo>) -> Option<Vec<FilenodeInfo>> {
        let res: Result<Vec<_>, _> = list.into_iter().map(FilenodeInfo::from_thrift).collect();
        if res.is_err() {
            STATS::gaf_deserialize_err.add_value(1);
        }
        res.ok()
    }

    let serialized = match memcache.get(keygen.key(&key.key)).await {
        Ok(Some(serialized)) => serialized,
        Ok(None) => {
            STATS::gaf_miss.add_value(1);
            return None;
        }
        Err(_) => {
            STATS::gaf_internal_err.add_value(1);
            return None;
        }
    };

    let thrift = match compact_protocol::deserialize(&serialized) {
        Ok(thrift) => thrift,
        Err(_) => {
            STATS::gaf_deserialize_err.add_value(1);
            return None;
        }
    };

    let res = match thrift {
        thrift::FilenodeInfoList::UnknownField(_) => {
            STATS::gaf_deserialize_err.add_value(1);
            return None;
        }
        thrift::FilenodeInfoList::Data(list) => {
            deserialize_list(list).map(RemoteCachedHistory::History)
        }
        thrift::FilenodeInfoList::Pointers(list) => {
            STATS::gaf_pointers.add_value(1);

            let read_chunks_fut = list.into_iter().map(move |pointer| {
                let chunk_key = get_mc_key_for_filenodes_list_chunk(keygen, key, pointer);

                async move {
                    match memcache.get(chunk_key).await {
                        Ok(Some(chunk)) => Ok(chunk),
                        _ => Err(()),
                    }
                }
            });

            let blob = match try_join_all(read_chunks_fut).await {
                Ok(chunks) => chunks
                    .into_iter()
                    .flat_map(|b| b.into_iter())
                    .collect::<Vec<u8>>(),
                Err(_) => {
                    STATS::gaf_pointers_err.add_value(1);
                    return None;
                }
            };

            match compact_protocol::deserialize(&blob) {
                Ok(thrift::FilenodeInfoList::Data(list)) => {
                    deserialize_list(list).map(RemoteCachedHistory::History)
                }
                Ok(thrift::FilenodeInfoList::TooBig(_)) => Some(RemoteCachedHistory::TooBig),
                _ => {
                    STATS::gaf_pointers_err.add_value(1);
                    None
                }
            }
        }
        thrift::FilenodeInfoList::TooBig(_) => Some(RemoteCachedHistory::TooBig),
    };

    if res.is_some() {
        STATS::gaf_hit.add_value(1);
    }

    res
}

fn schedule_fill_filenode(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<CachedFilenode>,
    filenode: FilenodeInfo,
) {
    let serialized = compact_protocol::serialize(&filenode.into_thrift());

    // Quite unlikely that single filenode will be bigger than MEMCACHE_VALUE_MAX_SIZE
    // It's probably not even worth logging it
    if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        let memcache = memcache.clone();
        let key = keygen.key(&key.key);
        let fut = async move {
            let _ = memcache.set(key, serialized).await;
        };

        tokio::spawn(fut);
    }
}

fn schedule_fill_history(
    memcache: MemcacheHandler,
    keygen: KeyGen,
    key: CacheKey<Option<CachedHistory>>,
    filenodes: Option<Vec<FilenodeInfo>>,
) {
    let fut = async move {
        let _ = fill_history(&memcache, &keygen, &key, filenodes).await;
    };

    tokio::spawn(fut);
}

fn serialize_history(filenodes: Option<Vec<FilenodeInfo>>) -> Bytes {
    let filenodes = match filenodes {
        Some(filenodes) => thrift::FilenodeInfoList::Data(
            filenodes
                .into_iter()
                .map(|filenode_info| filenode_info.into_thrift())
                .collect(),
        ),
        // Value in TooBig is ignored, so any value would work
        None => thrift::FilenodeInfoList::TooBig(0),
    };
    compact_protocol::serialize(&filenodes)
}

async fn fill_history(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<Option<CachedHistory>>,
    filenodes: Option<Vec<FilenodeInfo>>,
) -> Result<(), ()> {
    let serialized = serialize_history(filenodes);

    STATS::gaf_compact_bytes.add_value(serialized.len() as i64);

    let root = if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        serialized
    } else {
        let write_chunks_fut = serialized
            .chunks(MEMCACHE_VALUE_MAX_SIZE)
            .map(Vec::from) // takes ownership
            .zip(PointersIter::new())
            .map({
                move |(chunk, pointer)| {
                    async move {
                        let chunk_key = get_mc_key_for_filenodes_list_chunk(keygen, key, pointer);

                        // give chunks non-random max TTL_SEC_RAND so that they always live
                        // longer than the pointer
                        let chunk_ttl = Duration::from_secs(TTL_SEC + TTL_SEC_RAND);

                        memcache
                            .set_with_ttl(chunk_key, chunk, chunk_ttl)
                            .await
                            .map_err(drop)?;

                        Ok(pointer)
                    }
                }
            })
            .collect::<Vec<_>>();

        let pointers = try_join_all(write_chunks_fut).await?;
        compact_protocol::serialize(&thrift::FilenodeInfoList::Pointers(pointers))
    };

    let root_key = keygen.key(&key.key);
    let root_ttl = Duration::from_secs(TTL_SEC + random::<u64>() % TTL_SEC_RAND);

    memcache
        .set_with_ttl(root_key, root, root_ttl)
        .await
        .map_err(drop)?;

    Ok(())
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

#[cfg(test)]
pub mod test {
    use super::*;
    use anyhow::Error;
    use mercurial_types_mocks::nodehash::ONES_CSID;
    use mercurial_types_mocks::nodehash::ONES_FNID;
    use mononoke_types::RepoPath;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use path_hash::PathWithHash;
    use std::time::Duration;
    use tokio::time;

    use crate::reader::filenode_cache_key;
    use crate::reader::history_cache_key;

    const TIMEOUT_MS: u64 = 100;
    const SLEEP_MS: u64 = 5;

    fn filenode() -> FilenodeInfo {
        FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: Some((RepoPath::file("copiedfrom").unwrap(), ONES_FNID)),
            linknode: ONES_CSID,
        }
    }

    pub fn make_test_cache() -> RemoteCache {
        let keygen = KeyGen::new("newfilenodes.test", 0, 0);

        RemoteCache::Memcache(MemcacheCache {
            memcache: MemcacheHandler::create_mock(),
            keygen,
        })
    }

    pub async fn wait_for_filenode(
        cache: &RemoteCache,
        key: &CacheKey<CachedFilenode>,
    ) -> Result<FilenodeInfo, Error> {
        let r = time::timeout(Duration::from_millis(TIMEOUT_MS), async {
            loop {
                match cache.get_filenode(key).await {
                    Some(f) => {
                        break f;
                    }
                    None => {}
                }
                time::sleep(Duration::from_millis(SLEEP_MS)).await;
            }
        })
        .await?;

        Ok(r)
    }

    pub async fn wait_for_history(
        cache: &RemoteCache,
        key: &CacheKey<Option<CachedHistory>>,
    ) -> Result<Option<Vec<FilenodeInfo>>, Error> {
        let r = time::timeout(Duration::from_millis(TIMEOUT_MS), async {
            loop {
                match cache.get_history(key).await {
                    Some(f) => {
                        break f;
                    }
                    None => {}
                }
                time::sleep(Duration::from_millis(SLEEP_MS)).await;
            }
        })
        .await?;

        Ok(r.into_option())
    }

    #[fbinit::test]
    async fn test_store_filenode(_fb: FacebookInit) -> Result<(), Error> {
        let cache = make_test_cache();
        let path = RepoPath::file("copiedto")?;
        let info = filenode();

        let key = filenode_cache_key(
            REPO_ZERO,
            &PathWithHash::from_repo_path(&path),
            &info.filenode,
        );

        cache.fill_filenode(&key, info.clone());
        let from_cache = wait_for_filenode(&cache, &key).await?;

        assert_eq!(from_cache, info);

        Ok(())
    }

    #[fbinit::test]
    async fn test_store_short_history(_fb: FacebookInit) -> Result<(), Error> {
        let cache = make_test_cache();
        let path = RepoPath::file("copiedto")?;
        let info = filenode();
        let history = Some(vec![info.clone(), info.clone(), info.clone()]);

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);

        cache.fill_history(&key, history.clone());
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, history);

        Ok(())
    }

    #[fbinit::test]
    async fn test_store_long_history(_fb: FacebookInit) -> Result<(), Error> {
        let cache = make_test_cache();
        let path = RepoPath::file("copiedto")?;
        let info = filenode();

        let history = Some((0..100_000).map(|_| info.clone()).collect::<Vec<_>>());
        assert!(serialize_history(history.clone()).len() >= MEMCACHE_VALUE_MAX_SIZE);

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);

        cache.fill_history(&key, history.clone());
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, history);

        Ok(())
    }

    #[fbinit::test]
    async fn test_store_too_long_history(_fb: FacebookInit) -> Result<(), Error> {
        let cache = make_test_cache();
        let path = RepoPath::file("copiedto")?;

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);
        cache.fill_history(&key, None);
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, None);

        Ok(())
    }
}
