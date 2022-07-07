/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

#[derive(Args, Debug)]
pub struct CachelibArgs {
    /// Do not initialize cachelib and disable caches (useful for tests)
    #[clap(long)]
    pub skip_caching: bool,

    /// Run the blobstore with cachelib only (i.e., without memcache)
    #[clap(
        long,
        value_name = "BOOL",
        parse(try_from_str),
        default_missing_value = "true"
    )]
    pub blobstore_cachelib_only: bool,

    /// Number of shards to control concurrent access to a blobstore
    /// behind cachelib.
    #[clap(long, default_value = "0")]
    pub cachelib_shards: usize,

    /// Size of the cachelib cache, in GiB
    #[clap(long, value_name = "SIZE")]
    pub cache_size_gb: f64,

    /// Process size at which cachelib will shrink, in GiB
    #[clap(long)]
    pub max_process_size: Option<u32>,

    /// Process size at which cachelib will grow back to the
    /// value of --cache-size-gb, in GiB
    #[clap(long)]
    pub min_process_size: Option<u32>,

    /// Use the Tupperware-aware cache shrinker to avoid OOM
    #[clap(long)]
    pub use_tupperware_shrinker: bool,

    /// Ensure that objects of all size enjoy a similar LRU policy
    #[clap(long)]
    pub cachelib_rebalancing_use_lru: bool,

    /// How often to rebalance across allocation classes, in secs
    #[clap(long)]
    pub cachelib_rebalancing_interval_secs: u64,

    /// Override size of the blob cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub blob_cache_size: Option<usize>,

    /// Override size of the blob presence cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub presence_cache_size: Option<usize>,

    /// Override size of the changesets cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub changesets_cache_size: Option<usize>,

    /// Override size of the filenodes cache (individual filenodes)
    #[clap(long, value_name = "SIZE", hide = true)]
    pub filenodes_cache_size: Option<usize>,

    /// Override size of the filenodes history cache (batches of history)
    #[clap(long, value_name = "SIZE", hide = true)]
    pub filenodes_history_cache_size: Option<usize>,

    /// Override size of the hg mutation store cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub hg_mutation_store_cache_size: Option<usize>,

    /// Override size of the bonsai-hg mapping cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub idmapping_cache_size: Option<usize>,

    /// Override size of the bonsai-globalrevs mapping cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub globalrevs_cache_size: Option<usize>,

    /// Override size of the bonsai-svnrevs mapping cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub svnrevs_cache_size: Option<usize>,

    /// Override size of the phases cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub phases_cache_size: Option<usize>,

    /// Override size of the segmented changelog cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub segmented_changelog_cache_size: Option<usize>,

    /// Override size of the mutable renames cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub mutable_renames_cache_size: Option<usize>,

    /// Override the power for cachelib's hashtable buckets
    #[clap(long, value_name = "SIZE", hide = true)]
    pub buckets_power: Option<u32>,
}
