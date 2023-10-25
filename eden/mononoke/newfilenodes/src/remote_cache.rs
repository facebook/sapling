/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use bytes::Bytes;
use caching_ext::CacheHandlerFactory;
use caching_ext::MemcacheHandler;
use fbthrift::compact_protocol;
use filenodes::thrift;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRange;
use futures::future::try_join_all;
use memcache::KeyGen;
use memcache::MEMCACHE_VALUE_MAX_SIZE;
use rand::random;
use stats::prelude::*;
use time_ext::DurationExt;

use crate::local_cache::CacheKey;

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

const TTL_SEC: u64 = 8 * 60 * 60;

// Adding a random to TTL helps preventing eviction of all related keys at once
const TTL_SEC_RAND: u64 = 30 * 60; // 30min

pub struct RemoteCache {
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl RemoteCache {
    pub fn new(
        cache_handler_factory: &CacheHandlerFactory,
        backing_store_name: &str,
        backing_store_params: &str,
    ) -> Result<Self> {
        Ok(Self {
            memcache: cache_handler_factory.memcache(),
            keygen: Self::create_key_gen(backing_store_name, backing_store_params)?,
        })
    }

    pub fn new_noop() -> Result<Self> {
        Self::new(&CacheHandlerFactory::Noop, "newfilenodes", "")
    }

    #[cfg(test)]
    pub fn new_mock() -> Self {
        Self::new(&CacheHandlerFactory::Mocked, "newfilenodes", "test").unwrap()
    }

    fn create_key_gen(backing_store_name: &str, backing_store_params: &str) -> Result<KeyGen> {
        let key_prefix = format!(
            "scm.mononoke.filenodes.{}.{}",
            backing_store_name, backing_store_params,
        );

        let sitever = justknobs::get_as::<u32>("scm/mononoke_memcache_sitevers:filenodes", None)?;

        Ok(KeyGen::new(key_prefix, thrift::MC_CODEVER as u32, sitever))
    }

    // TODO: Can we optimize to reuse the existing PathWithHash we got?
    pub async fn get_filenode(&self, key: &CacheKey<FilenodeInfo>) -> Option<FilenodeInfo> {
        let now = Instant::now();

        let ret = get_single_filenode_from_memcache(&self.memcache, &self.keygen, key).await;

        let elapsed = now.elapsed().as_micros_unchecked() as i64;
        STATS::get_latency.add_value(elapsed);

        ret
    }

    // TODO: Need to use the same CacheKey here.
    pub fn fill_filenode(&self, key: &CacheKey<FilenodeInfo>, filenode: FilenodeInfo) {
        // Avoid wasting time spawning a fill operation if the memcache is a no-op
        if !self.memcache.is_noop() {
            schedule_fill_filenode(&self.memcache, &self.keygen, key, filenode);
        }
    }

    pub async fn get_history(&self, key: &CacheKey<FilenodeRange>) -> Option<FilenodeRange> {
        let now = Instant::now();

        let ret = get_history_from_memcache(&self.memcache, &self.keygen, key).await;

        let elapsed = now.elapsed().as_micros_unchecked() as i64;
        STATS::get_history.add_value(elapsed);

        ret
    }

    // TODO: Take ownership of key
    pub fn fill_history(&self, key: &CacheKey<FilenodeRange>, filenodes: FilenodeRange) {
        // Avoid wasting time spawning a fill operation if the memcache is a no-op
        if !self.memcache.is_noop() {
            schedule_fill_history(
                self.memcache.clone(),
                self.keygen.clone(),
                key.clone(),
                filenodes,
            );
        }
    }
}

type Pointer = i64;

fn get_mc_key_for_filenodes_list_chunk(
    keygen: &KeyGen,
    key: &CacheKey<FilenodeRange>,
    pointer: Pointer,
) -> String {
    keygen.key(format!("{}.{}", key.key, pointer))
}

async fn get_single_filenode_from_memcache(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<FilenodeInfo>,
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
    key: &CacheKey<FilenodeRange>,
) -> Option<FilenodeRange> {
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
            deserialize_list(list).map(FilenodeRange::Filenodes)
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
                    deserialize_list(list).map(FilenodeRange::Filenodes)
                }
                Ok(thrift::FilenodeInfoList::TooBig(_)) => Some(FilenodeRange::TooBig),
                _ => {
                    STATS::gaf_pointers_err.add_value(1);
                    None
                }
            }
        }
        thrift::FilenodeInfoList::TooBig(_) => Some(FilenodeRange::TooBig),
    };

    if res.is_some() {
        STATS::gaf_hit.add_value(1);
    }

    res
}

fn schedule_fill_filenode(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<FilenodeInfo>,
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
    key: CacheKey<FilenodeRange>,
    filenodes: FilenodeRange,
) {
    let fut = async move {
        let _ = fill_history(&memcache, &keygen, &key, filenodes).await;
    };

    tokio::spawn(fut);
}

fn serialize_history(filenodes: FilenodeRange) -> Bytes {
    let filenodes = match filenodes {
        FilenodeRange::Filenodes(filenodes) => thrift::FilenodeInfoList::Data(
            filenodes
                .into_iter()
                .map(|filenode_info| filenode_info.into_thrift())
                .collect(),
        ),
        // Value in TooBig is ignored, so any value would work
        FilenodeRange::TooBig => thrift::FilenodeInfoList::TooBig(0),
    };
    compact_protocol::serialize(&filenodes)
}

async fn fill_history(
    memcache: &MemcacheHandler,
    keygen: &KeyGen,
    key: &CacheKey<FilenodeRange>,
    filenodes: FilenodeRange,
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
    use std::time::Duration;

    use anyhow::Error;
    use fbinit::FacebookInit;
    use mercurial_types_mocks::nodehash::ONES_CSID;
    use mercurial_types_mocks::nodehash::ONES_FNID;
    use mononoke_types::RepoPath;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use path_hash::PathWithHash;
    use tokio::time;

    use super::*;
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

    pub async fn wait_for_filenode(
        cache: &RemoteCache,
        key: &CacheKey<FilenodeInfo>,
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
        key: &CacheKey<FilenodeRange>,
    ) -> Result<FilenodeRange, Error> {
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

        Ok(r)
    }

    #[fbinit::test]
    async fn test_store_filenode(_fb: FacebookInit) -> Result<(), Error> {
        let cache = RemoteCache::new_mock();
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
        let cache = RemoteCache::new_mock();
        let path = RepoPath::file("copiedto")?;
        let info = filenode();
        let history = FilenodeRange::Filenodes(vec![info.clone(), info.clone(), info.clone()]);

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);

        cache.fill_history(&key, history.clone());
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, history);

        Ok(())
    }

    #[fbinit::test]
    async fn test_store_long_history(_fb: FacebookInit) -> Result<(), Error> {
        let cache = RemoteCache::new_mock();
        let path = RepoPath::file("copiedto")?;
        let info = filenode();

        let history =
            FilenodeRange::Filenodes((0..100_000).map(|_| info.clone()).collect::<Vec<_>>());
        assert!(serialize_history(history.clone()).len() >= MEMCACHE_VALUE_MAX_SIZE);

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);

        cache.fill_history(&key, history.clone());
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, history);

        Ok(())
    }

    #[fbinit::test]
    async fn test_store_too_long_history(_fb: FacebookInit) -> Result<(), Error> {
        let cache = RemoteCache::new_mock();
        let path = RepoPath::file("copiedto")?;

        let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);
        cache.fill_history(&key, FilenodeRange::TooBig);
        let from_cache = wait_for_history(&cache, &key).await?;

        assert_eq!(from_cache, FilenodeRange::TooBig);

        Ok(())
    }
}
