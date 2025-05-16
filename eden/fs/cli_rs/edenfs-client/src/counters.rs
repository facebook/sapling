/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Counter names used for telemetry

// Filesystem counters (intially fuse, but will change to support other platforms)
const COUNTER_FUSE_OPEN: &str = "fuse.open_successful.sum";
const COUNTER_FUSE_READ: &str = "fuse.read_successful.sum";
const COUNTER_FUSE_READDIR: &str = "fuse.readdir_successful.sum";
const COUNTER_FUSE_WRITE: &str = "fuse.write_successful.sum";
const COUNTER_FUSE_GETATTR: &str = "fuse.getattr_successful.sum";

// EdenAPI backend counters
const COUNTER_EDENAPI_BLOBS_KEYS: &str = "scmstore.file.fetch.edenapi.keys";
const COUNTER_EDENAPI_BLOBS_REQUESTS: &str = "scmstore.file.fetch.edenapi.requests";
const COUNTER_EDENAPI_TREES_KEYS: &str = "scmstore.tree.fetch.edenapi.keys";
const COUNTER_EDENAPI_TREES_REQUESTS: &str = "scmstore.tree.fetch.edenapi.requests";

// LFS backend counters
const COUNTER_LFS_BLOBS_KEYS: &str = "scmstore.file.fetch.lfs.keys";
const COUNTER_LFS_BLOBS_REQUESTS: &str = "scmstore.file.fetch.lfs.requests";
const COUNTER_LFS_TREES_KEYS: &str = "scmstore.tree.fetch.lfs.keys";
const COUNTER_LFS_TREES_REQUESTS: &str = "scmstore.tree.fetch.lfs.requests";

// CAS backend counters
const COUNTER_CAS_BLOBS_HITS: &str = "scmstore.file.fetch.cas.hits";
const COUNTER_CAS_BLOBS_MISSES: &str = "scmstore.file.fetch.cas.misses";
const COUNTER_CAS_BLOBS_REQUESTS: &str = "scmstore.file.fetch.cas.requests";
const COUNTER_CAS_TREES_HITS: &str = "scmstore.tree.fetch.cas.hits";
const COUNTER_CAS_TREES_MISSES: &str = "scmstore.tree.fetch.cas.misses";
const COUNTER_CAS_TREES_REQUESTS: &str = "scmstore.tree.fetch.cas.requests";

// Sapling cache counters (known as indexedlog/hgcache)
const COUNTER_INDEXEDLOG_BLOBS_HITS: &str = "scmstore.file.fetch.indexedlog.cache.hits";
const COUNTER_INDEXEDLOG_BLOBS_MISSES: &str = "scmstore.file.fetch.indexedlog.cache.misses";
const COUNTER_INDEXEDLOG_BLOBS_REQUESTS: &str = "scmstore.file.fetch.indexedlog.cache.requests";
const COUNTER_INDEXEDLOG_TREES_HITS: &str = "scmstore.tree.fetch.indexedlog.cache.hits";
const COUNTER_INDEXEDLOG_TREES_MISSES: &str = "scmstore.tree.fetch.indexedlog.cache.misses";
const COUNTER_INDEXEDLOG_TREES_REQUESTS: &str = "scmstore.tree.fetch.indexedlog.cache.requests";

// Sapling LFS cache counters
const COUNTER_LFS_CACHE_BLOBS_KEYS: &str = "scmstore.file.fetch.lfs.cache.keys";
const COUNTER_LFS_CACHE_BLOBS_MISSES: &str = "scmstore.file.fetch.lfs.cache.misses";
const COUNTER_LFS_CACHE_BLOBS_REQUESTS: &str = "scmstore.file.fetch.lfs.cache.requests";

// RocksDB local store cache counters
const COUNTER_LOCAL_STORE_BLOBS_HITS: &str = "local_store.get_blob_success.sum";
const COUNTER_LOCAL_STORE_BLOBS_MISSES: &str = "local_store.get_blob_failure.sum";
const COUNTER_LOCAL_STORE_TREES_HITS: &str = "local_store.get_tree_success.sum";
const COUNTER_LOCAL_STORE_TREES_MISSES: &str = "local_store.get_tree_failure.sum";

// In-memory cache counters
const COUNTER_IN_MEMORY_BLOBS_HITS: &str = "blob_cache.get_hit.sum";
const COUNTER_IN_MEMORY_BLOBS_MISSES: &str = "blob_cache.get_miss.sum";
const COUNTER_IN_MEMORY_TREES_HITS: &str = "tree_cache.get_hit.sum";
const COUNTER_IN_MEMORY_TREES_MISSES: &str = "tree_cache.get_miss.sum";

// CAS local cache counters - file blobs
// Note: We don't have cas_direct.local_cache.misses, as these are generally retried via a non-direct code path.
const COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS: &str = "scmstore.file.fetch.cas.local_cache.hits.files";
const COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_HITS: &str =
    "scmstore.file.fetch.cas_direct.local_cache.hits.files";
const COUNTER_CAS_LOCAL_CACHE_BLOBS_MISSES: &str =
    "scmstore.file.fetch.cas.local_cache.misses.files";
const COUNTER_CAS_LOCAL_CACHE_BLOBS_LMDB_HITS: &str =
    "scmstore.file.fetch.cas.local_cache.lmdb.hits";
const COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_LMDB_HITS: &str =
    "scmstore.file.fetch.cas_direct.local_cache.lmdb.hits";
const COUNTER_CAS_LOCAL_CACHE_TREES_HITS: &str = "scmstore.tree.fetch.cas.local_cache.hits.files";
const COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_HITS: &str =
    "scmstore.tree.fetch.cas_direct.local_cache.hits.files";
const COUNTER_CAS_LOCAL_CACHE_TREES_MISSES: &str =
    "scmstore.tree.fetch.cas.local_cache.misses.files";
const COUNTER_CAS_LOCAL_CACHE_TREES_LMDB_HITS: &str =
    "scmstore.tree.fetch.cas.local_cache.lmdb.hits";
const COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_LMDB_HITS: &str =
    "scmstore.tree.fetch.cas_direct.local_cache.lmdb.hits";

use std::collections::BTreeMap;
use std::ops::Sub;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

#[derive(Debug, Clone, PartialEq, Eq)]
/// EdenFS filesystem counters
/// The exact VFS implementation depends on the platform
pub struct FilesystemTelemetryCounters {
    // The number of successful open filesystem operations.
    pub syscall_opens: i64,
    // The number of successful read operations.
    pub syscall_reads: i64,
    // The number of successful readdir operations.
    pub syscall_readdirs: i64,
    // The number of successful write operations.
    pub syscall_writes: i64,
    // The number of successful stat operations.
    pub syscall_stats: i64,
}

impl Sub for FilesystemTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            syscall_opens: self.syscall_opens - rhs.syscall_opens,
            syscall_reads: self.syscall_reads - rhs.syscall_reads,
            syscall_readdirs: self.syscall_readdirs - rhs.syscall_readdirs,
            syscall_writes: self.syscall_writes - rhs.syscall_writes,
            syscall_stats: self.syscall_stats - rhs.syscall_stats,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdenApiBackendTelemetryCounters {
    /// The number of file content fetches from the EdenAPI backend
    pub edenapi_fetches_blobs: i64,
    /// The number of tree fetches from the EdenAPI backend
    pub edenapi_fetches_trees: i64,
    /// Total number of http requests performed to the EdenAPI backend combined for files and trees
    pub edenapi_requests: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsBackendTelemetryCounters {
    /// The number of file content fetches from the LFS backend
    pub lfs_fetches_blobs: i64,
    /// Total number of http requests performed to the LFS backend combined for files and trees
    pub lfs_requests: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CASCBackendTelemetryCounters {
    /// The number of file content fetches from the CAS backend
    pub cas_fetches_blobs: i64,
    /// The number of file content fetches from the CAS backend that were not found
    pub cas_missing_blobs: i64,
    /// The number of tree fetches from the CAS backend
    pub cas_fetches_trees: i64,
    /// The number of tree fetches from the CAS backend that were not found
    pub cas_missing_trees: i64,
    /// Total number of requests performed to the CAS backend combined for files and trees
    pub cas_requests: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaplingCacheTelemetryCounters {
    // Blobs
    pub sapling_cache_blobs_hits: i64,
    pub sapling_cache_blobs_misses: i64,
    // Trees
    pub sapling_cache_trees_hits: i64,
    pub sapling_cache_trees_misses: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaplingLFSCacheTelemetryCounters {
    // Blobs
    pub sapling_lfs_cache_blobs_hits: i64,
    pub sapling_lfs_cache_blobs_misses: i64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CASCLocalCacheTelemetryCounters {
    // Blobs
    /// Total number of blobs fetched from the CAS local cache layers (on-disk cache and lmdb cache layer)
    pub cas_local_cache_blobs_hits: i64,
    /// Blobs fetched from lmdb cache layer
    pub cas_local_cache_blobs_lmdb_hits: i64,
    pub cas_local_cache_blobs_misses: i64,
    // Trees
    /// Total number of trees fetched from the CAS local cache layers (on-disk cache and lmdb cache layer)
    pub cas_local_cache_trees_hits: i64,
    /// Trees fetched from lmdb cache layer
    pub cas_local_cache_trees_lmdb_hits: i64,
    pub cas_local_cache_trees_misses: i64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStoreCacheTelemetryCounters {
    // Blobs
    pub local_store_cache_blobs_hits: i64,
    pub local_store_cache_blobs_misses: i64,
    // Trees
    pub local_store_cache_trees_hits: i64,
    pub local_store_cache_trees_misses: i64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemoryCacheTelemetryCounters {
    // Blobs
    pub in_memory_cache_blobs_hits: i64,
    pub in_memory_cache_blobs_misses: i64,
    // Trees
    pub in_memory_cache_trees_hits: i64,
    pub in_memory_cache_trees_misses: i64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// EdenFS cummulative counters
/// This is a subset of the counters that are available as part of the EdenFS telemetry
/// Only covers cumulative counters that are incremented on operations during the lifetime of the EdenFS daemon
/// It is possible to snapshot the counters and compare them to a previous snapshot
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryCounters {
    pub fs_stats: FilesystemTelemetryCounters,
    pub thrift_stats: ThriftTelemetryCounters,
    pub backend_stats: RemoteBackendTelemetryCounters,
    pub local_cache_stats: LocalCacheTelemetryCounters,
}

impl Sub for TelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            fs_stats: self.fs_stats - rhs.fs_stats,
            thrift_stats: self.thrift_stats - rhs.thrift_stats,
            backend_stats: self.backend_stats - rhs.backend_stats,
            local_cache_stats: self.local_cache_stats - rhs.local_cache_stats,
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
            COUNTER_FUSE_OPEN,
            COUNTER_FUSE_READ,
            COUNTER_FUSE_READDIR,
            COUNTER_FUSE_WRITE,
            COUNTER_FUSE_GETATTR,
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

        // Fetch the counters
        let counters = self.get_selected_counters(&keys).await?;

        // Create the TelemetryCounters struct
        let telemetry_counters = TelemetryCounters {
            fs_stats: FilesystemTelemetryCounters {
                syscall_opens: *counters.get(COUNTER_FUSE_OPEN).unwrap_or(&0),
                syscall_reads: *counters.get(COUNTER_FUSE_READ).unwrap_or(&0),
                syscall_readdirs: *counters.get(COUNTER_FUSE_READDIR).unwrap_or(&0),
                syscall_writes: *counters.get(COUNTER_FUSE_WRITE).unwrap_or(&0),
                syscall_stats: *counters.get(COUNTER_FUSE_GETATTR).unwrap_or(&0),
            },
            thrift_stats: ThriftTelemetryCounters {},
            backend_stats: RemoteBackendTelemetryCounters {
                edenapi_backend: Some(EdenApiBackendTelemetryCounters {
                    edenapi_fetches_blobs: *counters.get(COUNTER_EDENAPI_BLOBS_KEYS).unwrap_or(&0),
                    edenapi_fetches_trees: *counters.get(COUNTER_EDENAPI_TREES_KEYS).unwrap_or(&0),
                    edenapi_requests: *counters.get(COUNTER_EDENAPI_BLOBS_REQUESTS).unwrap_or(&0)
                        + *counters.get(COUNTER_EDENAPI_TREES_REQUESTS).unwrap_or(&0),
                }),
                casc_backend: Some(CASCBackendTelemetryCounters {
                    cas_fetches_blobs: *counters.get(COUNTER_CAS_BLOBS_HITS).unwrap_or(&0),
                    cas_missing_blobs: *counters.get(COUNTER_CAS_BLOBS_MISSES).unwrap_or(&0),
                    cas_fetches_trees: *counters.get(COUNTER_CAS_TREES_HITS).unwrap_or(&0),
                    cas_missing_trees: *counters.get(COUNTER_CAS_TREES_MISSES).unwrap_or(&0),
                    // Total number of requests performed to the CAS backend combined for files and trees
                    cas_requests: *counters.get(COUNTER_CAS_BLOBS_REQUESTS).unwrap_or(&0)
                        + *counters.get(COUNTER_CAS_TREES_REQUESTS).unwrap_or(&0),
                }),
                lfs_backend: Some(LfsBackendTelemetryCounters {
                    lfs_fetches_blobs: *counters.get(COUNTER_LFS_BLOBS_KEYS).unwrap_or(&0),
                    lfs_requests: *counters.get(COUNTER_LFS_BLOBS_REQUESTS).unwrap_or(&0)
                        + *counters.get(COUNTER_LFS_TREES_REQUESTS).unwrap_or(&0),
                }),
            },
            local_cache_stats: LocalCacheTelemetryCounters {
                sapling_cache: Some(SaplingCacheTelemetryCounters {
                    sapling_cache_blobs_hits: *counters
                        .get(COUNTER_INDEXEDLOG_BLOBS_HITS)
                        .unwrap_or(&0),
                    sapling_cache_blobs_misses: *counters
                        .get(COUNTER_INDEXEDLOG_BLOBS_MISSES)
                        .unwrap_or(&0),
                    sapling_cache_trees_hits: *counters
                        .get(COUNTER_INDEXEDLOG_TREES_HITS)
                        .unwrap_or(&0),
                    sapling_cache_trees_misses: *counters
                        .get(COUNTER_INDEXEDLOG_TREES_MISSES)
                        .unwrap_or(&0),
                }),
                sapling_lfs_cache: Some(SaplingLFSCacheTelemetryCounters {
                    sapling_lfs_cache_blobs_hits: *counters
                        .get(COUNTER_LFS_CACHE_BLOBS_KEYS)
                        .unwrap_or(&0)
                        - *counters.get(COUNTER_LFS_CACHE_BLOBS_MISSES).unwrap_or(&0),
                    sapling_lfs_cache_blobs_misses: *counters
                        .get(COUNTER_LFS_CACHE_BLOBS_MISSES)
                        .unwrap_or(&0),
                }),
                casc_local_cache: Some(CASCLocalCacheTelemetryCounters {
                    cas_local_cache_blobs_hits: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_HITS)
                            .unwrap_or(&0),
                    cas_local_cache_blobs_lmdb_hits: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_LMDB_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_LMDB_HITS)
                            .unwrap_or(&0),
                    cas_local_cache_blobs_misses: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_BLOBS_MISSES)
                        .unwrap_or(&0),
                    cas_local_cache_trees_hits: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_HITS)
                            .unwrap_or(&0),
                    cas_local_cache_trees_lmdb_hits: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_LMDB_HITS)
                        .unwrap_or(&0)
                        + *counters
                            .get(COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_LMDB_HITS)
                            .unwrap_or(&0),
                    cas_local_cache_trees_misses: *counters
                        .get(COUNTER_CAS_LOCAL_CACHE_TREES_MISSES)
                        .unwrap_or(&0),
                }),
                local_store_cache: Some(LocalStoreCacheTelemetryCounters {
                    local_store_cache_blobs_hits: *counters
                        .get(COUNTER_LOCAL_STORE_BLOBS_HITS)
                        .unwrap_or(&0),
                    local_store_cache_blobs_misses: *counters
                        .get(COUNTER_LOCAL_STORE_BLOBS_MISSES)
                        .unwrap_or(&0),
                    local_store_cache_trees_hits: *counters
                        .get(COUNTER_LOCAL_STORE_TREES_HITS)
                        .unwrap_or(&0),
                    local_store_cache_trees_misses: *counters
                        .get(COUNTER_LOCAL_STORE_TREES_MISSES)
                        .unwrap_or(&0),
                }),
                in_memory_local_cache: Some(InMemoryCacheTelemetryCounters {
                    in_memory_cache_blobs_hits: *counters
                        .get(COUNTER_IN_MEMORY_BLOBS_HITS)
                        .unwrap_or(&0),
                    in_memory_cache_blobs_misses: *counters
                        .get(COUNTER_IN_MEMORY_BLOBS_MISSES)
                        .unwrap_or(&0),
                    in_memory_cache_trees_hits: *counters
                        .get(COUNTER_IN_MEMORY_TREES_HITS)
                        .unwrap_or(&0),
                    in_memory_cache_trees_misses: *counters
                        .get(COUNTER_IN_MEMORY_TREES_MISSES)
                        .unwrap_or(&0),
                }),
            },
        };

        Ok(telemetry_counters)
    }

    /// Calculates the difference in EdenFS telemetry counters between the current state and a given initial state.
    ///
    /// This function retrieves the current telemetry counters and subtracts the provided initial counters from them.
    pub async fn get_telemetry_counter_delta(
        &self,
        initial_counters: TelemetryCounters,
    ) -> Result<TelemetryCounters> {
        let current_counters = self.get_telemetry_counters().await?;
        Ok(current_counters - initial_counters)
    }
}
