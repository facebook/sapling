/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

const ONE_GIB: usize = 1 << 30; // 2 ^ 30 = 1 GiB

#[derive(Clone)]
pub struct CachelibSettings {
    pub cache_size: usize,
    pub max_process_size_gib: Option<u32>,
    pub min_process_size_gib: Option<u32>,
    pub buckets_power: Option<u32>,
    pub use_tupperware_shrinker: bool,
    pub presence_cache_size: Option<usize>,
    pub changesets_cache_size: Option<usize>,
    pub filenodes_cache_size: Option<usize>,
    pub filenodes_history_cache_size: Option<usize>,
    pub idmapping_cache_size: Option<usize>,
    pub globalrev_cache_size: Option<usize>,
    pub svnrev_cache_size: Option<usize>,
    pub blob_cache_size: Option<usize>,
    pub phases_cache_size: Option<usize>,
    pub segmented_changelog_cache_size: Option<usize>,
    pub expected_item_size_bytes: Option<usize>,
    pub blobstore_cachelib_only: bool,
    pub rebalancing_use_lru: bool,
    pub rebalancing_interval: Duration,
}

impl Default for CachelibSettings {
    fn default() -> Self {
        Self {
            cache_size: 20 * ONE_GIB,
            max_process_size_gib: None,
            min_process_size_gib: None,
            buckets_power: None,
            use_tupperware_shrinker: false,
            presence_cache_size: None,
            changesets_cache_size: None,
            filenodes_cache_size: None,
            filenodes_history_cache_size: None,
            idmapping_cache_size: None,
            globalrev_cache_size: None,
            svnrev_cache_size: None,
            blob_cache_size: None,
            phases_cache_size: None,
            segmented_changelog_cache_size: None,
            expected_item_size_bytes: None,
            blobstore_cachelib_only: false,
            rebalancing_use_lru: false,
            rebalancing_interval: Duration::from_secs(300),
        }
    }
}
