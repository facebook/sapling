/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cached_config::ConfigStore;
use fbinit::FacebookInit;
pub struct Qps {}

impl Qps {
    pub fn new(
        _fb: FacebookInit,
        _top_level_tier: String,
        _config_store: &ConfigStore,
    ) -> Result<Qps, Error> {
        Ok(Self {})
    }

    pub fn bump(&self, _region: &str) -> Result<(), Error> {
        Ok(())
    }
}
