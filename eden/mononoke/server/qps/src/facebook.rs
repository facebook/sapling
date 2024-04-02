/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Error;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use common_server_region::RegionLong;
use common_server_region::RegionShort;
use fbinit::FacebookInit;
use fbthrift::ThriftEnum;
use fbwhoami::FbWhoAmI;
use ratelim::counter::Counter;
mod config;
use config::QpsConfig;

pub struct Qps {
    fb: FacebookInit,
    current_region_long: String,
    config_handle: ConfigHandle<QpsConfig>,
    short_to_long_region_mapping: HashMap<String, String>,
}

impl Qps {
    pub fn new(
        fb: FacebookInit,
        config_path: String,
        config_store: &ConfigStore,
    ) -> Result<Qps, Error> {
        let config_handle = config_store.get_config_handle_DEPRECATED(config_path)?;
        let short_to_long_region_mapping = create_short_to_long_region_mapping();

        let current_region_short = FbWhoAmI::get()?
            .region_datacenter_prefix
            .as_ref()
            .ok_or_else(|| anyhow!("Can't get 3-letter region datacenter prefix"))?
            .to_string();

        let current_region_long = short_to_long_region_mapping
            .get(&current_region_short)
            .ok_or_else(|| anyhow!("No such region {}", current_region_short))?
            .to_string();

        Ok(Self {
            fb,
            current_region_long,
            config_handle,
            short_to_long_region_mapping,
        })
    }

    pub fn bump(&self, region: &str) -> Result<(), Error> {
        let config = self.config_handle.get();
        let region_long = self
            .short_to_long_region_mapping
            .get(region)
            .ok_or_else(|| anyhow!("No such region {}", region))?;

        for ctr in config.counters_configs.iter() {
            let ctr = Counter::new(
                self.fb,
                &ctr.category,
                format!(
                    "{}:{}:{}:{}",
                    &ctr.prefix, &ctr.top_level_tier, region_long, &self.current_region_long
                ),
            );
            ctr.bump(1.0);
        }

        Ok(())
    }
}

fn create_short_to_long_region_mapping() -> HashMap<String, String> {
    let short_regions = RegionShort::variants();
    let long_regions = RegionLong::variants();
    short_regions
        .iter()
        .zip(long_regions.iter())
        .map(|(s, l)| (s.to_lowercase(), l.to_lowercase()))
        .collect()
}
