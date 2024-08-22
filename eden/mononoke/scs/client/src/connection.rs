/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Connection management.

use anyhow::Error;
use fbinit::FacebookInit;
use scs_client_raw::ScsClient;
use scs_client_raw::ScsClientBuilder;
use scs_client_raw::ScsClientHostBuilder;
use scs_client_raw::SCS_DEFAULT_TIER;

#[derive(clap::Args)]
pub(super) struct ConnectionArgs {
    #[clap(long, default_value = "scsc-default-client", global = true)]
    /// Name of the client for quota attribution and logging.
    client_id: String,
    #[clap(long, short, default_value = SCS_DEFAULT_TIER, global = true)]
    /// Connect to SCS through given tier.
    tier: String,
    #[clap(long, short = 'H', conflicts_with = "tier", global = true)]
    /// Connect to SCS through a given host and port pair, format HOST:PORT.
    host: Option<String>,
    #[clap(long, global = true)]
    processing_timeout: Option<u64>,
}

impl ConnectionArgs {
    pub fn get_connection(&self, fb: FacebookInit, repo: Option<&str>) -> Result<ScsClient, Error> {
        let disable_sr =
            std::env::var("MONONOKE_INTEGRATION_TEST_DISABLE_SR").map_or(false, |v| v == "true");
        if self.host.is_some() && disable_sr {
            ScsClientHostBuilder::new().build_from_host_port(fb, self.host.clone().unwrap())
        } else {
            ScsClientBuilder::new(fb, self.client_id.clone())
                .with_tier(&self.tier)
                .with_repo(repo.map(|r| r.to_string()))
                .with_host_and_port(self.host.clone())?
                .with_processing_timeout(self.processing_timeout.clone())
                .build()
        }
    }
}
