/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use clap::{ArgGroup, Args};

#[derive(Args)]
#[clap(group(ArgGroup::new("config").args(&["config-path", "config-tier", "prod"]).required(true)))]
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
}

fn configerator_config_path(tier: &str) -> String {
    format!("configerator://scm/mononoke/repos/tiers/{}", tier)
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
}
