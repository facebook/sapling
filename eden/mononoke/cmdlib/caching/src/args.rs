/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use arg_extensions::ArgDefaults;
use clap::Args;
use clap::ValueEnum;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMode {
    /// Caching is enabled, both local and shared caches.
    Enabled,

    /// Caching is enabled, but only the local cache is active.
    LocalOnly,

    /// Caching is disabled.
    Disabled,
}

impl ArgDefaults for CacheMode {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        vec![(
            "cache_mode",
            self.to_possible_value()
                .expect("default value must exist")
                .get_name()
                .to_string(),
        )]
    }
}

#[derive(Args, Debug)]
pub struct CachelibArgs {
    /// Mode to initialize caching in.
    #[clap(long, value_enum, default_value_t = CacheMode::Enabled)]
    pub cache_mode: CacheMode,

    #[clap(long)]
    /// Disable cacheadmin for cachelib
    pub cachelib_disable_cacheadmin: bool,

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

    /// Override size of the commit graph cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub commit_graph_cache_size: Option<usize>,

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

    /// Override size of the bonsai-git mapping cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub bonsai_git_mapping_cache_size: Option<usize>,

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

    /// Override size of the sql cache
    #[clap(long, value_name = "SIZE", hide = true)]
    pub sql_cache_size: Option<usize>,

    /// Override the power for cachelib's hashtable buckets
    #[clap(long, value_name = "SIZE", hide = true)]
    pub buckets_power: Option<u32>,
}
