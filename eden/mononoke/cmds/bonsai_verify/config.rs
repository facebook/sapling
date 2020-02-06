/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{Context, Result};
use clap::ArgMatches;
use mercurial_types::HgChangesetId;
use mononoke_types::DateTime;
use serde_derive::Deserialize;
use std::{fs::File, io::Read};
use toml::{self, value};

/// Configuration for the bonsai verify tool.
#[derive(Clone, Debug)]
pub struct Config {
    /// Which changesets are known to be broken and therefore should skip verification.
    pub ignores: Vec<HgChangesetId>,

    /// Old versions of treemanifest had a bug with merges: *some* p2 nodes would be set to null,
    /// while others would have the correct p2 manifest. This affects manifest hashes, but not
    /// correctness.
    ///
    /// If this is set and the commit time is before this, on a root manifest mismatch with a merge
    /// commit the verifier will check to see if there are any file changes. If there are, it will
    /// return "valid but different hash".
    ///
    /// Using the commit time is a somewhat unfortunate proxy for when that commit was converted
    /// to treemanifest, but it's the best we have.
    ///
    /// To treat all merges as potentially broken in this way, set this to a time in the future.
    pub broken_merges_before: Option<DateTime>,
}

impl Config {
    fn new(config_serde: ConfigSerde) -> Result<Self> {
        let broken_merges_before = match config_serde.broken_merges_before {
            Some(dt) => Some(
                DateTime::from_rfc3339(&dt.to_string())
                    .context("error while parsing broken_merges_before")?,
            ),
            None => None,
        };

        Ok(Self {
            ignores: config_serde.ignores,
            broken_merges_before,
        })
    }
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            ignores: vec![],
            broken_merges_before: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ConfigSerde {
    ignores: Vec<HgChangesetId>,
    broken_merges_before: Option<value::Datetime>,
}

pub fn get_config<'a>(matches: &ArgMatches<'a>) -> Result<Config> {
    let config_file = matches.value_of("config");
    let config_file = match config_file {
        Some(config_file) => config_file,
        None => return Ok(Config::default()),
    };

    let mut config_toml: Vec<u8> = vec![];
    let mut file = File::open(config_file)?;
    file.read_to_end(&mut config_toml)?;

    let config_serde: ConfigSerde =
        toml::from_slice(&config_toml).context("error while reading config")?;
    Config::new(config_serde)
}
