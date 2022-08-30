/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use once_cell::sync::Lazy;
use parking_lot::RwLock;

#[derive(PartialEq, Debug, Clone)]
pub struct Identity {
    cli_name: &'static str,
    product_name: &'static str,
    dot_dir: &'static str,
    env_prefix: &'static str,
    config_name: &'static str,
}

impl Identity {
    pub fn cli_name(&self) -> &'static str {
        self.cli_name
    }

    pub fn product_name(&self) -> &'static str {
        self.product_name
    }

    pub fn dot_dir(&self) -> &'static str {
        self.dot_dir
    }

    pub fn config_name(&self) -> &'static str {
        self.config_name
    }

    pub fn env_prefix(&self) -> &'static str {
        self.env_prefix
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cli_name)
    }
}

const HG: Identity = Identity {
    cli_name: "hg",
    product_name: "Mercurial",
    dot_dir: ".hg",
    env_prefix: "HG",
    config_name: "hgrc",
};

const SL: Identity = Identity {
    cli_name: "sl",
    product_name: "Sapling",
    dot_dir: ".sl",
    env_prefix: "SL",
    config_name: "slconfig",
};

const DEFAULT: Identity = HG;

static IDENTITY: Lazy<RwLock<Identity>> = Lazy::new(|| RwLock::new(DEFAULT));

/// CLI name to be used in user facing messaging.
pub fn cli_name() -> &'static str {
    IDENTITY.read().cli_name
}
