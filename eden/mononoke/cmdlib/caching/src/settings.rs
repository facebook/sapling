/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use super::args::CachelibArgs;

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
    pub hg_mutation_store_cache_size: Option<usize>,
    pub idmapping_cache_size: Option<usize>,
    pub globalrev_cache_size: Option<usize>,
    pub svnrev_cache_size: Option<usize>,
    pub blob_cache_size: Option<usize>,
    pub phases_cache_size: Option<usize>,
    pub segmented_changelog_cache_size: Option<usize>,
    pub mutable_renames_cache_size: Option<usize>,
    pub expected_item_size_bytes: Option<usize>,
    pub blobstore_cachelib_only: bool,
    pub rebalancing_use_lru: bool,
    pub rebalancing_interval: Duration,
}

impl CachelibSettings {
    pub fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        let mut defaults = vec![
            ("cache-size-gb", (self.cache_size / ONE_GIB).to_string()),
            (
                "cachelib-rebalancing-interval-secs",
                self.rebalancing_interval.as_secs().to_string(),
            ),
            (
                "blobstore-cachelib-only",
                self.blobstore_cachelib_only.to_string(),
            ),
        ];

        fn set_default<T: ToString>(
            defaults: &mut Vec<(&'static str, String)>,
            name: &'static str,
            value: &Option<T>,
        ) {
            if let Some(value) = value.as_ref() {
                defaults.push((name, value.to_string()));
            }
        }

        set_default(
            &mut defaults,
            "max-process-size",
            &self.max_process_size_gib,
        );
        set_default(
            &mut defaults,
            "min-process-size",
            &self.min_process_size_gib,
        );
        set_default(&mut defaults, "buckets-power", &self.buckets_power);
        set_default(
            &mut defaults,
            "presence-cache-size",
            &self.presence_cache_size,
        );
        set_default(
            &mut defaults,
            "changesets-cache-size",
            &self.changesets_cache_size,
        );
        set_default(
            &mut defaults,
            "filenodes-cache-size",
            &self.filenodes_cache_size,
        );
        set_default(
            &mut defaults,
            "filenodes-history-cache-size",
            &self.filenodes_history_cache_size,
        );
        set_default(
            &mut defaults,
            "hg-mutation-store-cache-size",
            &self.hg_mutation_store_cache_size,
        );
        set_default(
            &mut defaults,
            "idmapping-cache-size",
            &self.idmapping_cache_size,
        );
        set_default(
            &mut defaults,
            "globalrevs-cache-size",
            &self.globalrev_cache_size,
        );
        set_default(&mut defaults, "svnrevs-cache-size", &self.svnrev_cache_size);
        set_default(&mut defaults, "blob-cache-size", &self.blob_cache_size);
        set_default(&mut defaults, "phases-cache-size", &self.phases_cache_size);
        set_default(
            &mut defaults,
            "segmented-changelog-cache-size",
            &self.segmented_changelog_cache_size,
        );
        set_default(
            &mut defaults,
            "mutable-renames-cache-size",
            &self.mutable_renames_cache_size,
        );

        defaults
    }

    pub fn update_from_args(&mut self, args: &CachelibArgs) {
        self.cache_size = (args.cache_size_gb * ONE_GIB as f64) as usize;
        self.use_tupperware_shrinker = args.use_tupperware_shrinker;
        self.rebalancing_use_lru = args.cachelib_rebalancing_use_lru;
        self.rebalancing_interval = Duration::from_secs(args.cachelib_rebalancing_interval_secs);

        fn replace<T: Clone>(target: &mut Option<T>, value: &Option<T>) {
            if value.is_some() {
                *target = value.as_ref().cloned();
            }
        }

        replace(&mut self.max_process_size_gib, &args.max_process_size);
        replace(&mut self.min_process_size_gib, &args.min_process_size);
        replace(&mut self.buckets_power, &args.buckets_power);
        replace(&mut self.presence_cache_size, &args.presence_cache_size);
        replace(&mut self.changesets_cache_size, &args.changesets_cache_size);
        replace(&mut self.filenodes_cache_size, &args.filenodes_cache_size);
        replace(
            &mut self.filenodes_history_cache_size,
            &args.filenodes_history_cache_size,
        );
        replace(
            &mut self.hg_mutation_store_cache_size,
            &args.hg_mutation_store_cache_size,
        );
        replace(&mut self.idmapping_cache_size, &args.idmapping_cache_size);
        replace(&mut self.globalrev_cache_size, &args.globalrevs_cache_size);
        replace(&mut self.svnrev_cache_size, &args.svnrevs_cache_size);
        replace(&mut self.blob_cache_size, &args.blob_cache_size);
        replace(&mut self.phases_cache_size, &args.phases_cache_size);
        replace(
            &mut self.segmented_changelog_cache_size,
            &args.segmented_changelog_cache_size,
        );
        replace(
            &mut self.mutable_renames_cache_size,
            &args.mutable_renames_cache_size,
        );
    }
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
            hg_mutation_store_cache_size: None,
            idmapping_cache_size: None,
            globalrev_cache_size: None,
            svnrev_cache_size: None,
            blob_cache_size: None,
            phases_cache_size: None,
            segmented_changelog_cache_size: None,
            mutable_renames_cache_size: None,
            expected_item_size_bytes: None,
            blobstore_cachelib_only: false,
            rebalancing_use_lru: false,
            rebalancing_interval: Duration::from_secs(300),
        }
    }
}
