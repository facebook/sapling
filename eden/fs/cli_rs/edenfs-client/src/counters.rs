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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsBackendTelemetryCounters {
    /// The number of file content fetches from the LFS backend
    pub lfs_fetches_blobs: i64,
    /// The number of tree fetches from the LFS backend
    pub lfs_fetches_trees: i64,
    /// Total number of http requests performed to the LFS backend combined for files and trees
    pub lfs_requests: i64,
}

impl Sub for LfsBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            lfs_fetches_blobs: self.lfs_fetches_blobs - rhs.lfs_fetches_blobs,
            lfs_fetches_trees: self.lfs_fetches_trees - rhs.lfs_fetches_trees,
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
}

impl Sub for CASCBackendTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            cas_fetches_blobs: self.cas_fetches_blobs - rhs.cas_fetches_blobs,
            cas_missing_blobs: self.cas_missing_blobs - rhs.cas_missing_blobs,
            cas_fetches_trees: self.cas_fetches_trees - rhs.cas_fetches_trees,
            cas_missing_trees: self.cas_missing_trees - rhs.cas_missing_trees,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaplingLFSCacheTelemetryCounters {
    // Blobs
    pub sapling_lfs_cache_blobs_hits: i64,
    pub sapling_lfs_cache_blobs_misses: i64,
    // Trees
    pub sapling_lfs_cache_trees_hits: i64,
    pub sapling_lfs_cache_trees_misses: i64,
}
impl Sub for SaplingLFSCacheTelemetryCounters {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            sapling_lfs_cache_blobs_hits: self.sapling_lfs_cache_blobs_hits
                - rhs.sapling_lfs_cache_blobs_hits,
            sapling_lfs_cache_blobs_misses: self.sapling_lfs_cache_blobs_misses
                - rhs.sapling_lfs_cache_blobs_misses,
            sapling_lfs_cache_trees_hits: self.sapling_lfs_cache_trees_hits
                - rhs.sapling_lfs_cache_trees_hits,
            sapling_lfs_cache_trees_misses: self.sapling_lfs_cache_trees_misses
                - rhs.sapling_lfs_cache_trees_misses,
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
/// Only covers cumulative counters that are incremented on operations during the lifetime of the EdenFS process
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
}
