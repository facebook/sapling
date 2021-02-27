/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use structopt::StructOpt;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
/// Query Eden configuration
pub struct ConfigCmd {}

impl ConfigCmd {
    pub fn run(self, instance: EdenFsInstance) -> Result<ExitCode> {
        let config = match instance.get_config() {
            Ok(config) => config,
            Err(e) => {
                eprintln!("{}", e);
                return Ok(1);
            }
        };

        match toml::to_string_pretty(&config) {
            Ok(st) => {
                println!("{}", st);
                Ok(0)
            }
            Err(e) => {
                eprintln!("Error when seralizing configurations: {:?}", e);
                Ok(1)
            }
        }
    }
}
