/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use std::path::PathBuf;

#[cfg(windows)]
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
#[cfg(windows)]
use edenfs_utils::find_python;
use hg_util::path::expand_path;
use thrift_types::edenfs::types::GetConfigParams;
use thrift_types::edenfs_config::types::ConfigSourceType;

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
#[clap(about = "Reload EdenFS dynamic configs. This invokes edenfs_config_manager under the hood")]
pub struct ReloadConfigCmd {
    #[clap(
        short = 'n',
        long,
        help = "Dry run mode. Just print the config to stdout instead of writing it to disk"
    )]
    dry_run: bool,

    #[clap(
        long,
        help = "Log telemetry samples to a local file rather than to scuba (mainly for debugging and development)"
    )]
    local_telemetry: Option<PathBuf>,

    #[clap(long, parse(from_str = expand_path), help = "Write filtered config file to custom location")]
    out: Option<PathBuf>,

    #[clap(
        long,
        parse(from_str = expand_path),
        help = "Read and write location of the raw config which will be used if Configerator sends back an `edenfs_uptodate` repsonse"
    )]
    raw_out: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Number of seconds to wait for HTTP post response while fetching configs. Will use the edenfs_config_manager's default when this is not set (currently 5s as of Nov 2023, but that may have changed)."
    )]
    timeout: Option<u64>,

    #[clap(
        short = 'c',
        long,
        parse(from_str = expand_path),
        help = "Load configs from the given local configerator repo instead of reading from remote. This is useful for testing changes locally without having to push them to production"
    )]
    local_cfgr_root: Option<PathBuf>,

    #[clap(
        long,
        parse(from_str = expand_path),
        help = "Load configs from the given host instead of reading from remote. The specified host must have ran `arc canary` on itself prior to execution. This is useful for testing changes locally without having to push them to production"
    )]
    canary_host: Option<PathBuf>,

    #[clap(
        long,
        help = "If the script is ran as root, used to specify user when making requests to Configerator. Defaults to SUDO_USER, $LOGUSER, os.getlogin, or \"unknown\" in that order."
    )]
    user: Option<String>,

    #[clap(
        long,
        value_parser,
        // num_args = 1.., TODO(helsel): use num_args instead of value_delimiter once using clap 4
        value_delimiter = ',',
        help = "If the script is ran as root, user to use when making requests to Configerator. If given more than one value, defaults to using the first user as the Configerator requestor, but will log all users to Scuba for rollout tracking (defaults to SUDO_USER, $LOGUSER, os.getlogin, or \"unknown\" in that order)"
    )]
    users: Option<Vec<String>>,

    #[clap(short, long, help = "Enable more verbose console logging")]
    verbose: bool,
}

#[async_trait]
impl crate::Subcommand for ReloadConfigCmd {
    async fn run(&self) -> Result<ExitCode> {
        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut cmd_builder = std::process::Command::new("sudo");
            let edenfs_config_manager_cmd = "/usr/local/libexec/eden/edenfs_config_manager";
            cmd_builder.arg(edenfs_config_manager_cmd);
            cmd_builder
        };

        #[cfg(target_os = "windows")]
        let mut cmd = {
            let python = find_python().ok_or_else(|| anyhow!("Unable to find Python runtime"))?;
            let mut cmd_builder = std::process::Command::new(python);
            let edenfs_config_manager_cmd = r"c:\tools\eden\libexec\edenfs_config_manager.par";
            cmd_builder.arg(edenfs_config_manager_cmd);
            cmd_builder
        };

        if self.dry_run {
            cmd.arg("--dry-run");
        }

        if let Some(local_telemetry) = &self.local_telemetry {
            cmd.arg("--local-telemetry").arg(local_telemetry);
        }

        if let Some(out) = &self.out {
            cmd.arg("--out").arg(out);
        }

        if let Some(raw_out) = &self.raw_out {
            cmd.arg("--raw-out").arg(raw_out);
        }

        if let Some(timeout) = self.timeout {
            cmd.arg("--timeout").arg(timeout.to_string());
        }

        if let Some(local_cfgr_root) = &self.local_cfgr_root {
            cmd.arg("--local-cfgr-root").arg(local_cfgr_root);
        }

        if let Some(canary_host) = &self.canary_host {
            cmd.arg("--canary-host").arg(canary_host);
        }

        if let Some(user) = &self.user {
            cmd.arg("--user").arg(user);
        }

        if let Some(users) = &self.users {
            cmd.arg("--users").arg(
                users
                    .iter()
                    .map(|user| user.to_string() + " ")
                    .collect::<String>(),
            );
        }

        if self.verbose {
            cmd.arg("--verbose");
        }

        let status = cmd
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .expect("failed to execute edenfs_config_manager");

        if status.success() { Ok(0) } else { Ok(1) }
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
