/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::{Context, Result};
use structopt::{clap::AppSettings, StructOpt};
use tokio_compat_02::FutureExt;

use edenfs_client::EdenFsInstance;
use util::path::expand_path;

mod status;

const DEFAULT_CONFIG_DIR: &str = "~/local/.eden";
const DEFAULT_ETC_EDEN_DIR: &str = "/etc/eden";
const DEFAULT_HOME_DIR: &str = "~";

type ExitCode = i32;

fn expand_default(default: &'static str) -> impl Fn() -> PathBuf {
    move || expand_path(default)
}

#[derive(StructOpt, Debug)]
#[structopt(
    name = "edenfsctl",
    setting = AppSettings::DisableVersion,
    setting = AppSettings::DisableHelpFlags
)]
pub struct Command {
    /// The path to the directory where edenfs stores its internal state.
    #[structopt(long, parse(from_str = expand_path))]
    config_dir: Option<PathBuf>,

    /// Path to directory that holds the system configuration files.
    #[structopt(long, parse(from_str = expand_path))]
    etc_eden_dir: Option<PathBuf>,

    /// Path to directory where .edenrc config file is stored.
    #[structopt(long, parse(from_str = expand_path))]
    home_dir: Option<PathBuf>,

    #[structopt(subcommand)]
    subcommand: SubCommand,
}

#[derive(StructOpt, Debug)]
pub enum SubCommand {
    /// Check the health of the Eden service
    #[structopt(alias = "health")]
    Status(crate::status::StatusCmd),
}

impl Command {
    fn get_instance(&self) -> EdenFsInstance {
        EdenFsInstance::new(
            self.config_dir
                .clone()
                .unwrap_or_else(expand_default(DEFAULT_CONFIG_DIR)),
            self.etc_eden_dir
                .clone()
                .unwrap_or_else(expand_default(DEFAULT_ETC_EDEN_DIR)),
            self.home_dir
                .clone()
                .unwrap_or_else(expand_default(DEFAULT_HOME_DIR)),
        )
    }

    pub fn run(self) -> Result<ExitCode> {
        // For command line program, we don't really need concurrency. Schedule everything in
        // current thread should be sufficient.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("unable to start async runtime")?;

        Ok(runtime.block_on(self.dispatch().compat()))
    }

    /// Execute subcommands. This function returns only a return code since all the error handling
    /// should be taken care of by each sub-command.
    async fn dispatch(self) -> ExitCode {
        let instance = self.get_instance();
        match self.subcommand {
            SubCommand::Status(status) => status.run(instance).await,
        }
    }
}
