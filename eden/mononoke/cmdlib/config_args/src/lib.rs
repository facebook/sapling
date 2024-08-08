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
use slog::Logger;
use tokio::runtime::Handle;

#[derive(Args, Debug)]
#[clap(group(ArgGroup::new("config").args(&["config_path", "config_tier", "prod"]).required(true)))]
pub struct ConfigArgs {
    /// Path to Mononoke config
    #[clap(long, alias = "mononoke-config-path")]
    pub config_path: Option<String>,

    /// Use configerator-based configuration for a specific tier
    #[clap(long)]
    pub config_tier: Option<String>,

    /// Use configerator-based configuration for production
    #[clap(long)]
    pub prod: bool,

    /// Local path to fetch configerator configs from
    #[clap(long)]
    pub local_configerator_path: Option<PathBuf>,

    /// Regex for a Configerator path that must be covered by
    /// Mononoke's crypto project
    #[clap(long)]
    pub crypto_path_regex: Option<Vec<String>>,
}

const PRODUCTION_PREFIX: &str = "configerator://scm/mononoke/repos/tiers/";

fn configerator_config_path(tier: &str) -> String {
    format!("{}{}", PRODUCTION_PREFIX, tier)
}

impl ConfigArgs {
    pub fn config_path(&self) -> String {
        if let Some(config_path) = &self.config_path {
            config_path.clone()
        } else if self.prod {
            configerator_config_path("prod")
        } else if let Some(tier) = &self.config_tier {
            configerator_config_path(tier)
        } else {
            String::new()
        }
    }

    pub fn mode(&self) -> ConfigMode {
        if let Some(config_path) = &self.config_path {
            // Any configuration that matches the production prefix is prod.
            if config_path.starts_with(PRODUCTION_PREFIX) {
                return ConfigMode::Production;
            }
        } else {
            // Otherwise, we are prod if a prod tier is requested.
            if self.prod || self.config_tier.is_some() {
                return ConfigMode::Production;
            }
        }
        ConfigMode::Development
    }

    pub fn create_config_store(&self, fb: FacebookInit, logger: Logger) -> Result<ConfigStore> {
        const CRYPTO_PROJECT: &str = "SCM";
        const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
        const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

        if let Some(path) = &self.local_configerator_path {
            Ok(ConfigStore::file(
                logger,
                path.clone(),
                String::new(),
                CONFIGERATOR_POLL_INTERVAL,
            ))
        } else {
            let crypto_regex_paths = match &self.crypto_path_regex {
                Some(paths) => paths.clone(),
                None => vec![
                    "scm/mononoke/repos/.*".to_string(),
                    "scm/mononoke/redaction/.*".to_string(),
                ],
            };
            let crypto_regex = crypto_regex_paths
                .into_iter()
                .map(|path| (path, CRYPTO_PROJECT.to_string()))
                .collect();
            ConfigStore::regex_signed_configerator(
                fb,
                logger,
                crypto_regex,
                CONFIGERATOR_POLL_INTERVAL,
                CONFIGERATOR_REFRESH_TIMEOUT,
            )
        }
    }

    pub fn create_mononoke_configs(
        &self,
        handle: Handle,
        logger: Logger,
        config_store: &ConfigStore,
    ) -> Result<Arc<MononokeConfigs>> {
        let config_path = self.config_path();
        Ok(Arc::new(MononokeConfigs::new(
            config_path,
            config_store,
            handle,
            logger,
        )?))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConfigMode {
    Production,
    Development,
}
