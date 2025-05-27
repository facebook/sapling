/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::ops::Sub;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::counter_names::*;
use crate::methods::EdenThriftMethod;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// EdenFS filesystem counters
/// The exact VFS implementation depends on the platform
pub struct FilesystemTelemetryCounters {
    // The number of successful open filesystem operations.
    pub syscall_open: u64,
    // The number of successful read operations.
    pub syscall_read: u64,
    // The number of successful readdir operations.
    pub syscall_readdir: u64,
    // The number of successful readdirplus operations.
    pub syscall_readdirplus: u64,
    // The number of successful write operations.
    pub syscall_write: u64,
    // The number of successful stat operations.
    pub syscall_stat: u64,
    // The number of successful access operations.
    pub syscall_access: u64,
}

impl Sub for FilesystemTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            syscall_open: self.syscall_open - rhs.syscall_open,
            syscall_read: self.syscall_read - rhs.syscall_read,
            syscall_readdir: self.syscall_readdir - rhs.syscall_readdir,
            syscall_readdirplus: self.syscall_readdirplus - rhs.syscall_readdirplus,
            syscall_write: self.syscall_write - rhs.syscall_write,
            syscall_stat: self.syscall_stat - rhs.syscall_stat,
            syscall_access: self.syscall_access - rhs.syscall_access,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThriftTelemetryCounters {}

impl Sub for ThriftTelemetryCounters {
    type Output = Self;

    fn sub(self, _rhs: Self) -> Self::Output {
        Self {}
    }
}

/// Remote backends
/// EdenAPI backend counters
/// There are no misses as Mononoke is the source of truth for the data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdenApiBackendTelemetryCounters {
    /// The number of file content fetches from the EdenAPI backend
    pub edenapi_fetches_blobs: u64,
    /// The number of tree fetches from the EdenAPI backend
    pub edenapi_fetches_trees: u64,
    /// Total number of http requests performed to the EdenAPI backend combined for files and trees
    pub edenapi_requests: u64,
}

impl Sub for EdenApiBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            edenapi_fetches_blobs: self.edenapi_fetches_blobs - rhs.edenapi_fetches_blobs,
            edenapi_fetches_trees: self.edenapi_fetches_trees - rhs.edenapi_fetches_trees,
            edenapi_requests: self.edenapi_requests - rhs.edenapi_requests,
        }
    }
}

/// LFS backend counters
/// There are no misses as Mononoke is the source of truth for the data
/// LFS is not used for fetching trees
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsBackendTelemetryCounters {
    /// The number of file content fetches from the LFS backend
    pub lfs_fetches_blobs: u64,
    /// Total number of http requests performed to the LFS backend combined for files and trees
    pub lfs_requests: u64,
}

impl Sub for LfsBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            lfs_fetches_blobs: self.lfs_fetches_blobs - rhs.lfs_fetches_blobs,
            lfs_requests: self.lfs_requests - rhs.lfs_requests,
        }
    }
}

/// CASd backend counters
/// There could be misses as the storage layer is TTL based
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CASCBackendTelemetryCounters {
    /// The number of file content fetches from the CAS backend
    pub cas_fetches_blobs: u64,
    /// The number of file content fetches from the CAS backend that were not found
    pub cas_missing_blobs: u64,
    /// The number of tree fetches from the CAS backend
    pub cas_fetches_trees: u64,
    /// The number of tree fetches from the CAS backend that were not found
    pub cas_missing_trees: u64,
    /// Total number of requests performed to the CAS backend combined for files and trees
    pub cas_requests: u64,
}

impl Sub for CASCBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            cas_fetches_blobs: self.cas_fetches_blobs - rhs.cas_fetches_blobs,
            cas_missing_blobs: self.cas_missing_blobs - rhs.cas_missing_blobs,
            cas_fetches_trees: self.cas_fetches_trees - rhs.cas_fetches_trees,
            cas_missing_trees: self.cas_missing_trees - rhs.cas_missing_trees,
            cas_requests: self.cas_requests - rhs.cas_requests,
        }
    }
}

/// Remote backend counters to track the number of fetches from the remote backends
/// typically with much higher latency than the local caches
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteBackendTelemetryCounters {
    pub edenapi_backend: Option<EdenApiBackendTelemetryCounters>,
    pub casc_backend: Option<CASCBackendTelemetryCounters>,
    pub lfs_backend: Option<LfsBackendTelemetryCounters>,
}

impl Sub for RemoteBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            edenapi_backend: match (self.edenapi_backend, rhs.edenapi_backend) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            casc_backend: match (self.casc_backend, rhs.casc_backend) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            lfs_backend: match (self.lfs_backend, rhs.lfs_backend) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
        }
    }
}

/// Local caches (sapling "local" cache is skipped as it serves only a few fetches only for commits made locally)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaplingCacheTelemetryCounters {
    // Blobs
    pub sapling_cache_blobs_hits: u64,
    pub sapling_cache_blobs_misses: u64,
    // Trees
    pub sapling_cache_trees_hits: u64,
    pub sapling_cache_trees_misses: u64,
}

impl Sub for SaplingCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            sapling_cache_blobs_hits: self.sapling_cache_blobs_hits - rhs.sapling_cache_blobs_hits,
            sapling_cache_blobs_misses: self.sapling_cache_blobs_misses
                - rhs.sapling_cache_blobs_misses,
            sapling_cache_trees_hits: self.sapling_cache_trees_hits - rhs.sapling_cache_trees_hits,
            sapling_cache_trees_misses: self.sapling_cache_trees_misses
                - rhs.sapling_cache_trees_misses,
        }
    }
}

/// Sapling LFS Cache counters
/// The cache is only used for storing file content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaplingLFSCacheTelemetryCounters {
    // Blobs
    pub sapling_lfs_cache_blobs_hits: u64,
    pub sapling_lfs_cache_blobs_misses: u64,
}
impl Sub for SaplingLFSCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            sapling_lfs_cache_blobs_hits: self.sapling_lfs_cache_blobs_hits
                - rhs.sapling_lfs_cache_blobs_hits,
            sapling_lfs_cache_blobs_misses: self.sapling_lfs_cache_blobs_misses
                - rhs.sapling_lfs_cache_blobs_misses,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CASCLocalCacheTelemetryCounters {
    // Blobs
    /// Total number of blobs fetched from the CAS local cache layers (on-disk cache and lmdb cache layer)
    pub cas_local_cache_blobs_hits: u64,
    /// Blobs fetched from lmdb cache layer
    pub cas_local_cache_blobs_lmdb_hits: u64,
    pub cas_local_cache_blobs_misses: u64,
    // Trees
    /// Total number of trees fetched from the CAS local cache layers (on-disk cache and lmdb cache layer)
    pub cas_local_cache_trees_hits: u64,
    /// Trees fetched from lmdb cache layer
    pub cas_local_cache_trees_lmdb_hits: u64,
    pub cas_local_cache_trees_misses: u64,
}

impl Sub for CASCLocalCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            cas_local_cache_blobs_hits: self.cas_local_cache_blobs_hits
                - rhs.cas_local_cache_blobs_hits,
            cas_local_cache_blobs_lmdb_hits: self.cas_local_cache_blobs_lmdb_hits
                - rhs.cas_local_cache_blobs_lmdb_hits,
            cas_local_cache_blobs_misses: self.cas_local_cache_blobs_misses
                - rhs.cas_local_cache_blobs_misses,
            cas_local_cache_trees_hits: self.cas_local_cache_trees_hits
                - rhs.cas_local_cache_trees_hits,
            cas_local_cache_trees_lmdb_hits: self.cas_local_cache_trees_lmdb_hits
                - rhs.cas_local_cache_trees_lmdb_hits,
            cas_local_cache_trees_misses: self.cas_local_cache_trees_misses
                - rhs.cas_local_cache_trees_misses,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStoreCacheTelemetryCounters {
    // Blobs
    pub local_store_cache_blobs_hits: u64,
    pub local_store_cache_blobs_misses: u64,
    // Trees
    pub local_store_cache_trees_hits: u64,
    pub local_store_cache_trees_misses: u64,
}

impl Sub for LocalStoreCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            local_store_cache_blobs_hits: self.local_store_cache_blobs_hits
                - rhs.local_store_cache_blobs_hits,
            local_store_cache_blobs_misses: self.local_store_cache_blobs_misses
                - rhs.local_store_cache_blobs_misses,
            local_store_cache_trees_hits: self.local_store_cache_trees_hits
                - rhs.local_store_cache_trees_hits,
            local_store_cache_trees_misses: self.local_store_cache_trees_misses
                - rhs.local_store_cache_trees_misses,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InMemoryCacheTelemetryCounters {
    // Blobs
    pub in_memory_cache_blobs_hits: u64,
    pub in_memory_cache_blobs_misses: u64,
    // Trees
    pub in_memory_cache_trees_hits: u64,
    pub in_memory_cache_trees_misses: u64,
}

impl Sub for InMemoryCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            in_memory_cache_blobs_hits: self.in_memory_cache_blobs_hits
                - rhs.in_memory_cache_blobs_hits,
            in_memory_cache_blobs_misses: self.in_memory_cache_blobs_misses
                - rhs.in_memory_cache_blobs_misses,
            in_memory_cache_trees_hits: self.in_memory_cache_trees_hits
                - rhs.in_memory_cache_trees_hits,
            in_memory_cache_trees_misses: self.in_memory_cache_trees_misses
                - rhs.in_memory_cache_trees_misses,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCacheTelemetryCounters {
    /// Shared Sapling Cache counters (known also as hgcache)
    pub sapling_cache: Option<SaplingCacheTelemetryCounters>,
    /// Shared Sapling LFS Cache counters (known also as hgcache)
    pub sapling_lfs_cache: Option<SaplingLFSCacheTelemetryCounters>,
    /// CASd local cache counters
    pub casc_local_cache: Option<CASCLocalCacheTelemetryCounters>,
    /// Local store cache counters (eden rocksdb cache)
    pub local_store_cache: Option<LocalStoreCacheTelemetryCounters>,
    /// In memory (eden) cache counters
    pub in_memory_local_cache: Option<InMemoryCacheTelemetryCounters>,
}

impl Sub for LocalCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            sapling_cache: match (self.sapling_cache, rhs.sapling_cache) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            sapling_lfs_cache: match (self.sapling_lfs_cache, rhs.sapling_lfs_cache) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            casc_local_cache: match (self.casc_local_cache, rhs.casc_local_cache) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            local_store_cache: match (self.local_store_cache, rhs.local_store_cache) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            in_memory_local_cache: match (self.in_memory_local_cache, rhs.in_memory_local_cache) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
        }
    }
}

/// EdenFS file metadata telemetry
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadataTelemetry {
    // The number of times the file metadata was successfully fetched from the in-memory cache
    pub fetched_from_inmemory_cache: u64,
    // The number of times the file metadata was successfully fetched from the local store cache
    pub fetched_from_local_store_cache: u64,
    // The number of times the file metadata was successfully fetched from the backing store
    // This can come from both local backing store aux cache or remote if not found.
    pub fetched_from_backing_store: u64,
    // The number of times the file metadata was successfully fetched from the backing store but was cached.
    pub fetched_from_backing_store_cached: u64,
    // The number of times the file metadata was computed from the file content present in the backing store cache
    pub fetched_from_backing_store_computed: u64,
    // The number of times the file metadata was not found in any cache and had to be fetched from the remote
    pub fetched_from_remote: u64,
}

/// EdenFS tree metadata telemetry
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeMetadataTelemetry {
    // The number of times the tree metadata was successfully fetched from the in-memory cache
    pub fetched_from_inmemory_cache: u64,
    // The number of times the tree metadata was successfully fetched from the local store cache
    pub fetched_from_local_store_cache: u64,
    // The number of times the tree metadata was successfully fetched from the backing store
    pub fetched_from_backing_store: u64,
}

impl Sub for TreeMetadataTelemetry {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            fetched_from_inmemory_cache: self.fetched_from_inmemory_cache
                - rhs.fetched_from_inmemory_cache,
            fetched_from_local_store_cache: self.fetched_from_local_store_cache
                - rhs.fetched_from_local_store_cache,
            fetched_from_backing_store: self.fetched_from_backing_store
                - rhs.fetched_from_backing_store,
        }
    }
}

/// EdenFS cummulative counters
/// This is a subset of the counters that are available as part of the EdenFS telemetry
/// Only covers cumulative counters that are incremented on operations during the lifetime of the EdenFS daemon
/// It is possible to snapshot the counters and compare them to a previous snapshot
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryCounters {
    pub fs_stats: FilesystemTelemetryCounters,
    pub thrift_stats: ThriftTelemetryCounters,
    pub backend_stats: RemoteBackendTelemetryCounters,
    pub local_cache_stats: LocalCacheTelemetryCounters,
    pub file_metadata_stats: Option<FileMetadataTelemetry>,
    pub tree_metadata_stats: Option<TreeMetadataTelemetry>,
}

impl TelemetryCounters {
    /// Returns a CrawlingScore that aggregates the total amount of fetches from remote backends and local caches
    pub fn get_crawling_score(&self) -> CrawlingScore {
        CrawlingScore::from_telemetry(self)
    }

    /// Serialize the TelemetryCounters to a JSON string
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| EdenFsError::from(anyhow::anyhow!("Failed to serialize to JSON: {}", e)))
    }

    /// Serialize the TelemetryCounters to a pretty-printed JSON string
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EdenFsError::from(anyhow::anyhow!("Failed to serialize to JSON: {}", e)))
    }

    /// Deserialize a TelemetryCounters from a JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| {
            EdenFsError::from(anyhow::anyhow!("Failed to deserialize from JSON: {}", e))
        })
    }
}

impl Sub for FileMetadataTelemetry {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            fetched_from_inmemory_cache: self.fetched_from_inmemory_cache
                - rhs.fetched_from_inmemory_cache,
            fetched_from_local_store_cache: self.fetched_from_local_store_cache
                - rhs.fetched_from_local_store_cache,
            fetched_from_backing_store: self.fetched_from_backing_store
                - rhs.fetched_from_backing_store,
            fetched_from_backing_store_cached: self.fetched_from_backing_store_cached
                - rhs.fetched_from_backing_store_cached,
            fetched_from_backing_store_computed: self.fetched_from_backing_store_computed
                - rhs.fetched_from_backing_store_computed,
            fetched_from_remote: self.fetched_from_remote - rhs.fetched_from_remote,
        }
    }
}

/// CrawlingScore aggregates the total amount of fetches from remote backends and local caches hits, as well as amount of
/// filesystem operations performed during the workload.
/// This provides a summary of how much data was fetched during a workload
/// Please, note that this is approximate as if blob accessed multiple times during the workload
/// it will be counted every time it was accessed. The same applies to trees.
/// For example, read of a large file will be counted many times as the file is read in chunks.
/// For the remote fetches some fetches might be triggered by the prefetching logic and not on the critical path of the workload.
/// In this cases, they will be accounted as both remote and local cache fetches.
/// Finally, any data served from filesystem cache will not be accounted for as they do not come to EdenFS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlingScore {
    /// Total number of blob fetches from remote backends
    pub remote_blob_fetches: u64,
    /// Total number of tree fetches from remote backends
    pub remote_tree_fetches: u64,
    /// Total number of blob hits from local caches
    pub local_cache_blob_hits: u64,
    /// Total number of tree hits from local caches
    pub local_cache_tree_hits: u64,
    /// Total number of filesystem open and read operations
    pub fs_open_plus_read: u64,
    /// Total number of filesystem readdir operations (includes both readdir and readdirplus)
    pub fs_readdir: u64,
}

impl CrawlingScore {
    /// Creates a new CrawlingScore by aggregating data from TelemetryCounters
    pub fn from_telemetry(counters: &TelemetryCounters) -> Self {
        let mut remote_blob_fetches = 0;
        let mut remote_tree_fetches = 0;
        let mut local_cache_blob_hits = 0;
        let mut local_cache_tree_hits = 0;

        // Aggregate remote backend fetches
        if let Some(edenapi) = &counters.backend_stats.edenapi_backend {
            remote_blob_fetches += edenapi.edenapi_fetches_blobs;
            remote_tree_fetches += edenapi.edenapi_fetches_trees;
        }
        if let Some(casc) = &counters.backend_stats.casc_backend {
            remote_blob_fetches += casc.cas_fetches_blobs;
            remote_tree_fetches += casc.cas_fetches_trees;
        }
        if let Some(lfs) = &counters.backend_stats.lfs_backend {
            remote_blob_fetches += lfs.lfs_fetches_blobs;
        }

        // Aggregate local cache hits
        if let Some(sapling) = &counters.local_cache_stats.sapling_cache {
            local_cache_blob_hits += sapling.sapling_cache_blobs_hits;
            local_cache_tree_hits += sapling.sapling_cache_trees_hits;
        }
        if let Some(sapling_lfs) = &counters.local_cache_stats.sapling_lfs_cache {
            local_cache_blob_hits += sapling_lfs.sapling_lfs_cache_blobs_hits;
        }
        if let Some(casc_local) = &counters.local_cache_stats.casc_local_cache {
            local_cache_blob_hits += casc_local.cas_local_cache_blobs_hits;
            local_cache_tree_hits += casc_local.cas_local_cache_trees_hits;
        }
        if let Some(local_store) = &counters.local_cache_stats.local_store_cache {
            local_cache_blob_hits += local_store.local_store_cache_blobs_hits;
            local_cache_tree_hits += local_store.local_store_cache_trees_hits;
        }
        if let Some(in_memory) = &counters.local_cache_stats.in_memory_local_cache {
            local_cache_blob_hits += in_memory.in_memory_cache_blobs_hits;
            local_cache_tree_hits += in_memory.in_memory_cache_trees_hits;
        }

        // Get filesystem operations
        let fs_open_plus_read = counters.fs_stats.syscall_open + counters.fs_stats.syscall_read;
        let fs_readdir = counters.fs_stats.syscall_readdir + counters.fs_stats.syscall_readdirplus;

        Self {
            remote_blob_fetches,
            remote_tree_fetches,
            local_cache_blob_hits,
            local_cache_tree_hits,
            fs_open_plus_read,
            fs_readdir,
        }
    }

    /// Returns the total number of remote fetches (blobs + trees)
    pub fn total_remote_fetches(&self) -> u64 {
        self.remote_blob_fetches + self.remote_tree_fetches
    }

    /// Returns the total number of local cache hits (blobs + trees)
    pub fn total_local_cache_hits(&self) -> u64 {
        self.local_cache_blob_hits + self.local_cache_tree_hits
    }

    /// Returns the total number of fetches and hits (remote + local cache)
    pub fn total_access_score(&self) -> u64 {
        self.total_remote_fetches() + self.total_local_cache_hits()
    }

    /// Returns the total number of filesystem operations (open + read + readdir)
    pub fn total_filesystem_ops(&self) -> u64 {
        self.fs_open_plus_read + self.fs_readdir
    }

    /// Serialize the CrawlingScore to a JSON string
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| EdenFsError::from(anyhow::anyhow!("Failed to serialize to JSON: {}", e)))
    }

    /// Serialize the CrawlingScore to a pretty-printed JSON string
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EdenFsError::from(anyhow::anyhow!("Failed to serialize to JSON: {}", e)))
    }
}

impl Sub for CrawlingScore {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            remote_blob_fetches: self.remote_blob_fetches - rhs.remote_blob_fetches,
            remote_tree_fetches: self.remote_tree_fetches - rhs.remote_tree_fetches,
            local_cache_blob_hits: self.local_cache_blob_hits - rhs.local_cache_blob_hits,
            local_cache_tree_hits: self.local_cache_tree_hits - rhs.local_cache_tree_hits,
            fs_open_plus_read: self.fs_open_plus_read - rhs.fs_open_plus_read,
            fs_readdir: self.fs_readdir - rhs.fs_readdir,
        }
    }
}

impl Sub for TelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            fs_stats: self.fs_stats - rhs.fs_stats,
            thrift_stats: self.thrift_stats - rhs.thrift_stats,
            backend_stats: self.backend_stats - rhs.backend_stats,
            local_cache_stats: self.local_cache_stats - rhs.local_cache_stats,
            file_metadata_stats: match (self.file_metadata_stats, rhs.file_metadata_stats) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
            tree_metadata_stats: match (self.tree_metadata_stats, rhs.tree_metadata_stats) {
                (Some(lhs), Some(rhs)) => Some(lhs - rhs),
                (lhs, None) => lhs,
                (None, _) => None,
            },
        }
    }
}

impl EdenFsClient {
    pub async fn get_regex_counters(&self, arg_regex: &str) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| {
            (
                thrift.getRegexCounters(arg_regex),
                EdenThriftMethod::GetRegexCounters,
            )
        })
        .await
        .with_context(|| "failed to get regex counters")
        .map_err(EdenFsError::from)
    }

    pub async fn get_selected_counters(&self, keys: &[String]) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| {
            (
                thrift.getSelectedCounters(keys),
                EdenThriftMethod::GetSelectedCounters,
            )
        })
        .await
        .with_context(|| "failed to get selected counters")
        .map_err(EdenFsError::from)
    }

    pub async fn get_counters(&self) -> Result<BTreeMap<String, i64>> {
        self.with_thrift(|thrift| (thrift.getCounters(), EdenThriftMethod::GetCounters))
            .await
            .with_context(|| "failed to get counters")
            .map_err(EdenFsError::from)
    }

    pub async fn get_counter(&self, key: &str) -> Result<i64> {
        self.with_thrift(|thrift| (thrift.getCounter(key), EdenThriftMethod::GetCounter))
            .await
            .with_context(|| format!("failed to get counter for key {}", key))
            .map_err(EdenFsError::from)
    }

    /// Fetch telemetry counters from EdenFS and return them as a TelemetryCounters struct.
    /// This method fetches all the counters needed to fill the TelemetryCounters struct.
    ///
    /// The counters returned are cumulative counters for the lifetime of the EdenFS process.
    /// Please use the `get_telemetry_counter_delta` method using
    /// with a snapshot of the counters collected at the beginning of your workflow using `get_telemetry_counters`
    pub async fn get_telemetry_counters(&self) -> Result<TelemetryCounters> {
        // Define the counter keys we need to fetch, organized by category
        let filesystem_counters = [
            COUNTER_FS_OPEN,
            COUNTER_FS_READ,
            COUNTER_FS_READDIR,
            COUNTER_FS_READDIRPLUS,
            COUNTER_FS_WRITE,
            COUNTER_FS_GETATTR,
            COUNTER_FS_ACCESS,
        ];

        let cas_local_cache_counters = [
            COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS,
            COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_HITS,
            COUNTER_CAS_LOCAL_CACHE_BLOBS_MISSES,
            COUNTER_CAS_LOCAL_CACHE_TREES_HITS,
            COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_HITS,
            COUNTER_CAS_LOCAL_CACHE_TREES_MISSES,
            COUNTER_CAS_LOCAL_CACHE_BLOBS_LMDB_HITS,
            COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_LMDB_HITS,
            COUNTER_CAS_LOCAL_CACHE_TREES_LMDB_HITS,
            COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_LMDB_HITS,
        ];

        let edenapi_backend_counters = [
            COUNTER_EDENAPI_BLOBS_KEYS,
            COUNTER_EDENAPI_TREES_KEYS,
            COUNTER_EDENAPI_BLOBS_REQUESTS,
            COUNTER_EDENAPI_TREES_REQUESTS,
        ];

        let lfs_backend_counters = [COUNTER_LFS_BLOBS_KEYS, COUNTER_LFS_BLOBS_REQUESTS];

        let cas_backend_counters = [
            COUNTER_CAS_BLOBS_HITS,
            COUNTER_CAS_BLOBS_MISSES,
            COUNTER_CAS_BLOBS_REQUESTS,
            COUNTER_CAS_TREES_HITS,
            COUNTER_CAS_TREES_MISSES,
            COUNTER_CAS_TREES_REQUESTS,
        ];

        let indexedlog_cache_counters = [
            COUNTER_INDEXEDLOG_BLOBS_HITS,
            COUNTER_INDEXEDLOG_BLOBS_MISSES,
            COUNTER_INDEXEDLOG_BLOBS_REQUESTS,
            COUNTER_INDEXEDLOG_TREES_HITS,
            COUNTER_INDEXEDLOG_TREES_MISSES,
            COUNTER_INDEXEDLOG_TREES_REQUESTS,
        ];

        let lfs_cache_counters = [
            COUNTER_LFS_CACHE_BLOBS_KEYS,
            COUNTER_LFS_CACHE_BLOBS_MISSES,
            COUNTER_LFS_CACHE_BLOBS_REQUESTS,
        ];

        let local_store_cache_counters = [
            COUNTER_LOCAL_STORE_BLOBS_HITS,
            COUNTER_LOCAL_STORE_BLOBS_MISSES,
            COUNTER_LOCAL_STORE_TREES_HITS,
            COUNTER_LOCAL_STORE_TREES_MISSES,
        ];

        let in_memory_cache_counters = [
            COUNTER_IN_MEMORY_BLOBS_HITS,
            COUNTER_IN_MEMORY_BLOBS_MISSES,
            COUNTER_IN_MEMORY_TREES_HITS,
            COUNTER_IN_MEMORY_TREES_MISSES,
        ];

        let file_metadata_counters = [
            COUNTER_METADATA_MEMORY,
            COUNTER_METADATA_LOCAL_STORE,
            COUNTER_METADATA_BACKING_STORE,
            COUNTER_METADATA_AUX_COMPUTED,
            COUNTER_METADATA_AUX_HITS,
            COUNTER_METADATA_AUX_MISSES,
        ];

        let tree_metadata_counters = [
            COUNTER_TREE_METADATA_MEMORY,
            COUNTER_TREE_METADATA_LOCAL_STORE,
            COUNTER_TREE_METADATA_BACKING_STORE,
        ];

        // Combine all counter keys into a single vector
        let mut keys: Vec<String> = Vec::new();
        keys.extend(filesystem_counters.iter().map(|&s| s.to_string()));
        keys.extend(cas_local_cache_counters.iter().map(|&s| s.to_string()));
        keys.extend(edenapi_backend_counters.iter().map(|&s| s.to_string()));
        keys.extend(lfs_backend_counters.iter().map(|&s| s.to_string()));
        keys.extend(cas_backend_counters.iter().map(|&s| s.to_string()));
        keys.extend(indexedlog_cache_counters.iter().map(|&s| s.to_string()));
        keys.extend(lfs_cache_counters.iter().map(|&s| s.to_string()));
        keys.extend(local_store_cache_counters.iter().map(|&s| s.to_string()));
        keys.extend(in_memory_cache_counters.iter().map(|&s| s.to_string()));
        keys.extend(file_metadata_counters.iter().map(|&s| s.to_string()));
        keys.extend(tree_metadata_counters.iter().map(|&s| s.to_string()));

        // Fetch the counters
        let counters = self.get_selected_counters(&keys).await?;

        // Create the TelemetryCounters struct
        let telemetry_counters = TelemetryCounters {
            fs_stats: FilesystemTelemetryCounters {
                syscall_open: *counters.get(COUNTER_FS_OPEN).unwrap_or(&0) as u64,
                syscall_read: *counters.get(COUNTER_FS_READ).unwrap_or(&0) as u64,
                syscall_readdir: *counters.get(COUNTER_FS_READDIR).unwrap_or(&0) as u64,
                syscall_readdirplus: *counters.get(COUNTER_FS_READDIRPLUS).unwrap_or(&0) as u64,
                syscall_write: *counters.get(COUNTER_FS_WRITE).unwrap_or(&0) as u64,
                syscall_stat: *counters.get(COUNTER_FS_GETATTR).unwrap_or(&0) as u64,
                syscall_access: *counters.get(COUNTER_FS_ACCESS).unwrap_or(&0) as u64,
            },
            thrift_stats: ThriftTelemetryCounters {},
            backend_stats: RemoteBackendTelemetryCounters {
                edenapi_backend: Some(EdenApiBackendTelemetryCounters {
                    edenapi_fetches_blobs: *counters.get(COUNTER_EDENAPI_BLOBS_KEYS).unwrap_or(&0)
                        as u64,
                    edenapi_fetches_trees: *counters.get(COUNTER_EDENAPI_TREES_KEYS).unwrap_or(&0)
                        as u64,
                    edenapi_requests: (*counters.get(COUNTER_EDENAPI_BLOBS_REQUESTS).unwrap_or(&0)
                        + *counters.get(COUNTER_EDENAPI_TREES_REQUESTS).unwrap_or(&0))
                        as u64,
                }),
                casc_backend: Some(CASCBackendTelemetryCounters {
                    cas_fetches_blobs: *counters.get(COUNTER_CAS_BLOBS_HITS).unwrap_or(&0) as u64
                        - *counters
                            .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS)
                            .unwrap_or(&0) as u64,
                    cas_missing_blobs: *counters.get(COUNTER_CAS_BLOBS_MISSES).unwrap_or(&0) as u64,
                    cas_fetches_trees: *counters.get(COUNTER_CAS_TREES_HITS).unwrap_or(&0) as u64
                        - *counters
                            .get(COUNTER_CAS_LOCAL_CACHE_TREES_HITS)
                            .unwrap_or(&0) as u64,
                    cas_missing_trees: *counters.get(COUNTER_CAS_TREES_MISSES).unwrap_or(&0) as u64,
                    // Total number of requests performed to the CAS backend combined for files and trees
                    cas_requests: (*counters.get(COUNTER_CAS_BLOBS_REQUESTS).unwrap_or(&0)
                        + *counters.get(COUNTER_CAS_TREES_REQUESTS).unwrap_or(&0))
                        as u64,
                }),
                lfs_backend: Some(LfsBackendTelemetryCounters {
                    lfs_fetches_blobs: *counters.get(COUNTER_LFS_BLOBS_KEYS).unwrap_or(&0) as u64,
                    lfs_requests: *counters.get(COUNTER_LFS_BLOBS_REQUESTS).unwrap_or(&0) as u64,
                }),
            },
            local_cache_stats: LocalCacheTelemetryCounters {
                sapling_cache: Some(SaplingCacheTelemetryCounters {
                    sapling_cache_blobs_hits: *counters
                        .get(COUNTER_INDEXEDLOG_BLOBS_HITS)
                        .unwrap_or(&0) as u64,
                    sapling_cache_blobs_misses: *counters
                        .get(COUNTER_INDEXEDLOG_BLOBS_MISSES)
                        .unwrap_or(&0) as u64,
                    sapling_cache_trees_hits: *counters
                        .get(COUNTER_INDEXEDLOG_TREES_HITS)
                        .unwrap_or(&0) as u64,
                    sapling_cache_trees_misses: *counters
                        .get(COUNTER_INDEXEDLOG_TREES_MISSES)
                        .unwrap_or(&0) as u64,
                }),
                sapling_lfs_cache: Some(SaplingLFSCacheTelemetryCounters {
                    sapling_lfs_cache_blobs_hits: (*counters
                        .get(COUNTER_LFS_CACHE_BLOBS_KEYS)
                        .unwrap_or(&0)
                        - *counters.get(COUNTER_LFS_CACHE_BLOBS_MISSES).unwrap_or(&0))
                        as u64,
                    sapling_lfs_cache_blobs_misses: *counters
                        .get(COUNTER_LFS_CACHE_BLOBS_MISSES)
                        .unwrap_or(&0) as u64,
                }),
                casc_local_cache: Some(CASCLocalCacheTelemetryCounters {
                    cas_local_cache_blobs_hits: (*counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_HITS)
                            .unwrap_or(&0)) as u64,
                    cas_local_cache_blobs_lmdb_hits: (*counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_LMDB_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_LMDB_HITS)
                            .unwrap_or(&0))
                        as u64,
                    cas_local_cache_blobs_misses: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_MISSES)
                        .unwrap_or(&0) as u64,
                    cas_local_cache_trees_hits: (*counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_HITS)
                            .unwrap_or(&0)) as u64,
                    cas_local_cache_trees_lmdb_hits: (*counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_LMDB_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_LMDB_HITS)
                            .unwrap_or(&0))
                        as u64,
                    cas_local_cache_trees_misses: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_MISSES)
                        .unwrap_or(&0) as u64,
                }),
                local_store_cache: Some(LocalStoreCacheTelemetryCounters {
                    local_store_cache_blobs_hits: *counters
                        .get(COUNTER_LOCAL_STORE_BLOBS_HITS)
                        .unwrap_or(&0) as u64,
                    local_store_cache_blobs_misses: *counters
                        .get(COUNTER_LOCAL_STORE_BLOBS_MISSES)
                        .unwrap_or(&0) as u64,
                    local_store_cache_trees_hits: *counters
                        .get(COUNTER_LOCAL_STORE_TREES_HITS)
                        .unwrap_or(&0) as u64,
                    local_store_cache_trees_misses: *counters
                        .get(COUNTER_LOCAL_STORE_TREES_MISSES)
                        .unwrap_or(&0) as u64,
                }),
                in_memory_local_cache: Some(InMemoryCacheTelemetryCounters {
                    in_memory_cache_blobs_hits: *counters
                        .get(COUNTER_IN_MEMORY_BLOBS_HITS)
                        .unwrap_or(&0) as u64,
                    in_memory_cache_blobs_misses: *counters
                        .get(COUNTER_IN_MEMORY_BLOBS_MISSES)
                        .unwrap_or(&0) as u64,
                    in_memory_cache_trees_hits: *counters
                        .get(COUNTER_IN_MEMORY_TREES_HITS)
                        .unwrap_or(&0) as u64,
                    in_memory_cache_trees_misses: *counters
                        .get(COUNTER_IN_MEMORY_TREES_MISSES)
                        .unwrap_or(&0) as u64,
                }),
            },
            file_metadata_stats: Some(FileMetadataTelemetry {
                fetched_from_inmemory_cache: *counters.get(COUNTER_METADATA_MEMORY).unwrap_or(&0)
                    as u64,
                fetched_from_local_store_cache: *counters
                    .get(COUNTER_METADATA_LOCAL_STORE)
                    .unwrap_or(&0) as u64,
                fetched_from_backing_store: *counters
                    .get(COUNTER_METADATA_BACKING_STORE)
                    .unwrap_or(&0) as u64,
                fetched_from_backing_store_cached: *counters
                    .get(COUNTER_METADATA_AUX_HITS)
                    .unwrap_or(&0) as u64,
                fetched_from_backing_store_computed: *counters
                    .get(COUNTER_METADATA_AUX_COMPUTED)
                    .unwrap_or(&0) as u64,
                fetched_from_remote: *counters.get(COUNTER_METADATA_AUX_MISSES).unwrap_or(&0)
                    as u64,
            }),
            tree_metadata_stats: Some(TreeMetadataTelemetry {
                fetched_from_inmemory_cache: *counters
                    .get(COUNTER_TREE_METADATA_MEMORY)
                    .unwrap_or(&0) as u64,
                fetched_from_local_store_cache: *counters
                    .get(COUNTER_TREE_METADATA_LOCAL_STORE)
                    .unwrap_or(&0) as u64,
                fetched_from_backing_store: *counters
                    .get(COUNTER_TREE_METADATA_BACKING_STORE)
                    .unwrap_or(&0) as u64,
            }),
        };

        Ok(telemetry_counters)
    }

    /// Calculates the difference in EdenFS telemetry counters between the current state and a given initial state.
    ///
    /// This function retrieves the current telemetry counters and subtracts the provided initial counters from them.
    pub async fn get_telemetry_counters_delta(
        &self,
        initial_counters: TelemetryCounters,
    ) -> Result<TelemetryCounters> {
        let current_counters = self.get_telemetry_counters().await?;
        Ok(current_counters - initial_counters)
    }

    /// Calculates the EdenFS crawling score delta between the current state and a given initial state.
    pub async fn get_crawling_score_delta(
        &self,
        initial_counters: TelemetryCounters,
    ) -> Result<CrawlingScore> {
        let counters = self.get_telemetry_counters_delta(initial_counters).await?;
        Ok(counters.get_crawling_score())
    }
}
