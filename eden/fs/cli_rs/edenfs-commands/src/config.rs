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
use thrift_types::edenfs::types::GetConfigParams;
use thrift_types::edenfs_config::types::ConfigSourceType;
use thrift_types::edenfs_config::types::ConfigValue;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Query EdenFS CLI configuration")]
pub struct CliConfigCmd {}

#[async_trait]
impl crate::Subcommand for CliConfigCmd {
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

#[derive(Parser, Debug)]
#[clap(about = "Query EdenFS daemon configuration")]
pub struct FsConfigCmd {
    #[clap(long, help = "Show all, including defaulted, configuration values")]
    all: bool,
}

#[async_trait]
impl crate::Subcommand for FsConfigCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let client = instance.connect(None).await?;

        let params: GetConfigParams = Default::default();
        let config_data = client.getConfig(&params).await?;

        let mut current_section: Option<String> = None;

        for (key, value) in config_data.values {
            if !self.all && value.sourceType == ConfigSourceType::Default {
                continue;
            }

            let (section, name) = match key.split_once(':') {
                Some(pair) => pair,
                None => continue,
            };
            let cs = Some(section.to_string());
            if current_section != cs {
                if current_section.is_some() {
                    println!();
                }
                println!("[{}]", section);
                current_section = cs;
            }

            let str = format!("{} = \"{}\"", name, value.parsedValue);
            if value.sourcePath.is_empty() {
                println!("{}", str);
            } else {
                const SOURCE_COLUMN: usize = 39;
                let white = if str.len() >= SOURCE_COLUMN {
                    1
                } else {
                    SOURCE_COLUMN - str.len()
                };
                // It's a little bit easier to mentally separate the value
                // from the path with no whitespace between the comment hash
                // and the path.
                println!(
                    "{}{: <2$}# {3}",
                    str,
                    "",
                    white,
                    String::from_utf8_lossy(&value.sourcePath)
                );
            }
        }

        Ok(0)
    }
}
