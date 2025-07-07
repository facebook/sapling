/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Counter names used for telemetry

// Filesystem counters
#[cfg(target_os = "macos")]
pub mod fs_counters {
    pub const COUNTER_FS_OPEN: &str = "nfs.open_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_READ: &str = "nfs.read_successful.sum";
    pub const COUNTER_FS_READDIR: &str = "nfs.readdir_successful.sum";
    pub const COUNTER_FS_READDIRPLUS: &str = "nfs.readdirplus_successful.sum";
    pub const COUNTER_FS_WRITE: &str = "nfs.write_successful.sum";
    pub const COUNTER_FS_GETATTR: &str = "nfs.getattr_successful.sum";
    pub const COUNTER_FS_SETATTR: &str = "nfs.setattr_successful.sum";
    pub const COUNTER_FS_LOOKUP: &str = "nfs.lookup_successful.sum";
    pub const COUNTER_FS_ACCESS: &str = "nfs.access_successful.sum";
    pub const COUNTER_FS_MKDIR: &str = "nfs.mkdir_successful.sum";
}

#[cfg(target_os = "windows")]
pub mod fs_counters {
    pub const COUNTER_FS_OPEN: &str = "prjfs.open_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_READ: &str = "prjfs.read_successful.sum";
    pub const COUNTER_FS_READDIR: &str = "prjfs.readdir_successful.sum";
    pub const COUNTER_FS_READDIRPLUS: &str = "prjfs.readdirplus_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_WRITE: &str = "prjfs.write_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_GETATTR: &str = "prjfs.getattr_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_SETATTR: &str = "prjfs.setattr_successful.sum"; // placeholder, does not exist
    pub const COUNTER_FS_LOOKUP: &str = "prjfs.lookup_successful.sum";
    pub const COUNTER_FS_ACCESS: &str = "prjfs.access_successful.sum";
    pub const COUNTER_FS_MKDIR: &str = "prjfs.mkdir_successful.sum"; // placeholder, does not exist
}

// Filesystem counters for FUSE
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub mod fs_counters {
    pub const COUNTER_FS_OPEN: &str = "fuse.open_successful.sum";
    pub const COUNTER_FS_READ: &str = "fuse.read_successful.sum";
    pub const COUNTER_FS_READDIR: &str = "fuse.readdir_successful.sum";
    pub const COUNTER_FS_READDIRPLUS: &str = "fuse.readdirplus_successful.sum"; // not used
    pub const COUNTER_FS_WRITE: &str = "fuse.write_successful.sum";
    pub const COUNTER_FS_GETATTR: &str = "fuse.getattr_successful.sum";
    pub const COUNTER_FS_SETATTR: &str = "fuse.setattr_successful.sum";
    pub const COUNTER_FS_LOOKUP: &str = "fuse.lookup_successful.sum";
    pub const COUNTER_FS_ACCESS: &str = "fuse.access_successful.sum";
    pub const COUNTER_FS_MKDIR: &str = "fuse.mkdir_successful.sum";
}

// Re-export the filesystem counters
pub use fs_counters::*;

// EdenAPI backend counters
pub const COUNTER_EDENAPI_BLOBS_KEYS: &str = "scmstore.file.fetch.edenapi.keys";
pub const COUNTER_EDENAPI_BLOBS_REQUESTS: &str = "scmstore.file.fetch.edenapi.requests";
pub const COUNTER_EDENAPI_TREES_KEYS: &str = "scmstore.tree.fetch.edenapi.keys";
pub const COUNTER_EDENAPI_TREES_REQUESTS: &str = "scmstore.tree.fetch.edenapi.requests";

// LFS backend counters
pub const COUNTER_LFS_BLOBS_KEYS: &str = "scmstore.file.fetch.lfs.keys";
pub const COUNTER_LFS_BLOBS_REQUESTS: &str = "scmstore.file.fetch.lfs.requests";

// CAS backend counters
pub const COUNTER_CAS_BLOBS_HITS: &str = "scmstore.file.fetch.cas.hits";
pub const COUNTER_CAS_BLOBS_MISSES: &str = "scmstore.file.fetch.cas.misses";
pub const COUNTER_CAS_BLOBS_REQUESTS: &str = "scmstore.file.fetch.cas.requests";
pub const COUNTER_CAS_TREES_HITS: &str = "scmstore.tree.fetch.cas.hits";
pub const COUNTER_CAS_TREES_MISSES: &str = "scmstore.tree.fetch.cas.misses";
pub const COUNTER_CAS_TREES_REQUESTS: &str = "scmstore.tree.fetch.cas.requests";

// Sapling cache counters (known as indexedlog/hgcache)
pub const COUNTER_INDEXEDLOG_BLOBS_HITS: &str = "scmstore.file.fetch.indexedlog.cache.hits";
pub const COUNTER_INDEXEDLOG_BLOBS_MISSES: &str = "scmstore.file.fetch.indexedlog.cache.misses";
pub const COUNTER_INDEXEDLOG_BLOBS_REQUESTS: &str = "scmstore.file.fetch.indexedlog.cache.requests";
pub const COUNTER_INDEXEDLOG_TREES_HITS: &str = "scmstore.tree.fetch.indexedlog.cache.hits";
pub const COUNTER_INDEXEDLOG_TREES_MISSES: &str = "scmstore.tree.fetch.indexedlog.cache.misses";
pub const COUNTER_INDEXEDLOG_TREES_REQUESTS: &str = "scmstore.tree.fetch.indexedlog.cache.requests";

// Sapling LFS cache counters
pub const COUNTER_LFS_CACHE_BLOBS_KEYS: &str = "scmstore.file.fetch.lfs.cache.keys";
pub const COUNTER_LFS_CACHE_BLOBS_MISSES: &str = "scmstore.file.fetch.lfs.cache.misses";
pub const COUNTER_LFS_CACHE_BLOBS_REQUESTS: &str = "scmstore.file.fetch.lfs.cache.requests";

// RocksDB local store cache counters
pub const COUNTER_LOCAL_STORE_BLOBS_HITS: &str = "local_store.get_blob_success.sum";
pub const COUNTER_LOCAL_STORE_BLOBS_MISSES: &str = "local_store.get_blob_failure.sum";
pub const COUNTER_LOCAL_STORE_TREES_HITS: &str = "local_store.get_tree_success.sum";
pub const COUNTER_LOCAL_STORE_TREES_MISSES: &str = "local_store.get_tree_failure.sum";

// In-memory cache counters
pub const COUNTER_IN_MEMORY_BLOBS_HITS: &str = "blob_cache.get_hit.sum";
pub const COUNTER_IN_MEMORY_BLOBS_MISSES: &str = "blob_cache.get_miss.sum";
pub const COUNTER_IN_MEMORY_TREES_HITS: &str = "tree_cache.get_hit.sum";
pub const COUNTER_IN_MEMORY_TREES_MISSES: &str = "tree_cache.get_miss.sum";

// File metadata counters
pub const COUNTER_METADATA_MEMORY: &str = "object_store.get_blob_metadata.memory.count";
pub const COUNTER_METADATA_LOCAL_STORE: &str = "object_store.get_blob_metadata.local_store.count";
pub const COUNTER_METADATA_BACKING_STORE: &str =
    "object_store.get_blob_metadata.backing_store.count";
pub const COUNTER_METADATA_AUX_COMPUTED: &str = "scmstore.file.fetch.aux.cache.computed";
pub const COUNTER_METADATA_AUX_HITS: &str = "scmstore.file.fetch.aux.cache.hits";
pub const COUNTER_METADATA_AUX_MISSES: &str = "scmstore.file.fetch.aux.cache.misses";

// Tree metadata counters
pub const COUNTER_TREE_METADATA_MEMORY: &str = "object_store.get_tree_metadata.memory.count";
pub const COUNTER_TREE_METADATA_LOCAL_STORE: &str =
    "object_store.get_tree_metadata.local_store.count";
pub const COUNTER_TREE_METADATA_BACKING_STORE: &str =
    "object_store.get_tree_metadata.backing_store.count";

// CAS local cache counters - file blobs
// Note: We don't have cas_direct.local_cache.misses, as these are generally retried via a non-direct code path.
pub const COUNTER_CAS_LOCAL_CACHE_BLOBS_HITS: &str =
    "scmstore.file.fetch.cas.local_cache.hits.files";
pub const COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_HITS: &str =
    "scmstore.file.fetch.cas_direct.local_cache.hits.files";
pub const COUNTER_CAS_LOCAL_CACHE_BLOBS_MISSES: &str =
    "scmstore.file.fetch.cas.local_cache.misses.files";
pub const COUNTER_CAS_LOCAL_CACHE_BLOBS_LMDB_HITS: &str =
    "scmstore.file.fetch.cas.local_cache.lmdb.hits";
pub const COUNTER_CAS_DIRECT_LOCAL_CACHE_BLOBS_LMDB_HITS: &str =
    "scmstore.file.fetch.cas_direct.local_cache.lmdb.hits";
pub const COUNTER_CAS_LOCAL_CACHE_TREES_HITS: &str =
    "scmstore.tree.fetch.cas.local_cache.hits.files";
pub const COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_HITS: &str =
    "scmstore.tree.fetch.cas_direct.local_cache.hits.files";
pub const COUNTER_CAS_LOCAL_CACHE_TREES_MISSES: &str =
    "scmstore.tree.fetch.cas.local_cache.misses.files";
pub const COUNTER_CAS_LOCAL_CACHE_TREES_LMDB_HITS: &str =
    "scmstore.tree.fetch.cas.local_cache.lmdb.hits";
pub const COUNTER_CAS_DIRECT_LOCAL_CACHE_TREES_LMDB_HITS: &str =
    "scmstore.tree.fetch.cas_direct.local_cache.lmdb.hits";
