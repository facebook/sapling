/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for controlling tunables
#[derive(Args, Debug)]
pub struct TunablesArgs {
    /// Tunables dynamic config path in Configerator
    #[clap(long)]
    pub tunables_config: Option<String>,

    /// Tunables static config local path
    #[clap(long, conflicts_with = "tunables-config")]
    pub tunables_local_path: Option<String>,

    /// Use the default values for all tunables (useful for tests)
    #[clap(long, conflicts_with_all = &["tunables-config", "tunables-local-path"])]
    pub disable_tunables: bool,
}

const DEFAULT_TUNABLES_CONFIG: &str = "scm/mononoke/tunables/default";

impl TunablesArgs {
    pub fn tunables_config_or_default(&self) -> String {
        self.tunables_config
            .clone()
            .unwrap_or_else(|| DEFAULT_TUNABLES_CONFIG.to_string())
    }
}
