/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cachelib_cache;
pub use crate::cachelib_cache::{
    new_cachelib_blobstore, new_cachelib_blobstore_no_lease, CachelibBlobstoreOptions,
};

pub mod dummy;

mod in_process_lease;
pub use in_process_lease::InProcessLease;

mod locking_cache;
pub use crate::locking_cache::{CacheBlobstore, CacheBlobstoreExt, CacheOps, LeaseOps};

mod memcache_cache_lease;
pub use crate::memcache_cache_lease::{
    new_memcache_blobstore, new_memcache_blobstore_no_lease, MemcacheOps,
};

mod mem_writes;
pub use crate::mem_writes::MemWritesBlobstore;
