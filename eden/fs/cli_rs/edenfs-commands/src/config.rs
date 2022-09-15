/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Query EdenFS configuration")]
pub struct ConfigCmd {}

#[async_trait]
impl crate::Subcommand for ConfigCmd {
    async fn run(&self) -> Result<ExitCode> {
        let config = match EdenFsInstance::global().get_config() {
            Ok(config) => config,
            Err(e) => {
                eprintln!("{}", e);
                return Ok(1);
            }
        };

        match toml::to_string(&config) {
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
