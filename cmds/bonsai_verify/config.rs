// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fs::File;
use std::io::Read;

use clap::ArgMatches;
use failure::Result;
use toml;

use mercurial_types::HgChangesetId;

/// Configuration for the bonsai verify tool.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub ignores: Vec<HgChangesetId>,
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self { ignores: vec![] }
    }
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

    Ok(toml::from_slice(&config_toml)?)
}
