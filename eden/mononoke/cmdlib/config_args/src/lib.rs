/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use cached_config::ConfigStore;
use clap::ArgGroup;
use clap::Args;
use fbinit::FacebookInit;
use mononoke_configs::ConfigLocation;
use mononoke_configs::MononokeConfigs;
use mononoke_configs::tier_manifest_path;
use tokio::runtime::Handle;

#[derive(Args, Debug)]
#[clap(group(ArgGroup::new("config").args(&["config_path", "config_tier", "prod", "git_config"]).required(true)))]
pub struct ConfigArgs {
    /// Local Mononoke config: a TOML directory or a JSON file
    #[clap(long, alias = "mononoke-config-path")]
    pub config_path: Option<String>,

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
    /// The configerator tier, or `None` for a local `--config-path`.
    pub fn tier_name(&self) -> Option<&str> {
        if self.prod {
            Some("prod")
        } else if self.git_config {
            Some("gitimport_content")
        } else {
            self.config_tier.as_deref()
        }
    }

    /// Where the configs come from, for display: the tier manifest's
    /// configerator path, or the local path.
    pub fn config_path(&self) -> String {
        match (self.tier_name(), &self.config_path) {
            (Some(tier), _) => tier_manifest_path(tier),
            (None, Some(path)) => path.clone(),
            (None, None) => String::new(),
        }
    }

    pub fn mode(&self) -> ConfigMode {
        if self.tier_name().is_some() {
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
        let location = match (self.tier_name(), &self.config_path) {
            (Some(tier), _) => ConfigLocation::Tier(tier),
            (None, Some(path)) => ConfigLocation::Path(Path::new(path)),
            (None, None) => anyhow::bail!(
                "one of --config-path, --config-tier, --prod or --git-config is required"
            ),
        };
        Ok(Arc::new(MononokeConfigs::new(
            location,
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
