/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use cached_config::ConfigStore;
use clap::ArgGroup;
use clap::Args;
use fbinit::FacebookInit;
use mononoke_configs::MononokeConfigs;
use mononoke_configs::tier_manifest_path;
use tokio::runtime::Handle;

#[derive(Args, Debug)]
#[clap(group(ArgGroup::new("config").args(&["config_tier", "prod", "git_config"]).required(true)))]
pub struct ConfigArgs {
    /// Use configerator-based configuration for a specific tier
    #[clap(long)]
    pub config_tier: Option<String>,

    /// Use configerator-based configuration for production
    #[clap(long)]
    pub prod: bool,

    /// Use configerator-based configuration for git repos
    #[clap(long)]
    pub git_config: bool,

    /// Local path to fetch configerator configs from
    #[clap(long)]
    pub local_configerator_path: Option<PathBuf>,
}

impl ConfigArgs {
    /// The configerator tier; clap guarantees one of the three flags is set.
    pub fn tier_name(&self) -> &str {
        if self.prod {
            "prod"
        } else if self.git_config {
            "gitimport_content"
        } else {
            self.config_tier
                .as_deref()
                .expect("clap requires one of --config-tier, --prod or --git-config")
        }
    }

    /// The configerator path of the tier manifest, for display.
    pub fn config_path(&self) -> String {
        tier_manifest_path(self.tier_name())
    }

    /// Production means live configerator. A tier served from
    /// `--local-configerator-path` is a development setup (the integration
    /// tests, mononoke-bootstrap, Mononoke-on-AWS).
    pub fn mode(&self) -> ConfigMode {
        if self.local_configerator_path.is_none() {
            ConfigMode::Production
        } else {
            ConfigMode::Development
        }
    }

    pub fn create_config_store(&self, fb: FacebookInit) -> Result<ConfigStore> {
        const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
        const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

        if let Some(path) = &self.local_configerator_path {
            Ok(ConfigStore::file(
                true,
                path.clone(),
                String::new(),
                CONFIGERATOR_POLL_INTERVAL,
            ))
        } else {
            ConfigStore::configerator(fb, true, true, CONFIGERATOR_REFRESH_TIMEOUT)
        }
    }

    pub fn create_mononoke_configs(
        &self,
        handle: Handle,
        config_store: &ConfigStore,
    ) -> Result<Arc<MononokeConfigs>> {
        Ok(Arc::new(MononokeConfigs::new(
            self.tier_name(),
            config_store,
            handle,
        )?))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConfigMode {
    Production,
    Development,
}
