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
use tracing::{event, Level};

use edenfs_client::EdenFsInstance;
use util::path::expand_path;

mod pid;
mod status;

#[cfg(unix)]
const DEFAULT_CONFIG_DIR: &str = "~/local/.eden";
#[cfg(unix)]
const DEFAULT_ETC_EDEN_DIR: &str = "/etc/eden";

#[cfg(windows)]
const DEFAULT_CONFIG_DIR: &str = "~/.eden";
#[cfg(windows)]
const DEFAULT_ETC_EDEN_DIR: &str = "C:\\ProgramData\\facebook\\eden";

type ExitCode = i32;

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

    #[structopt(long)]
    pub debug: bool,

    #[structopt(subcommand)]
    subcommand: SubCommand,
}

#[derive(StructOpt, Debug)]
pub enum SubCommand {
    /// Check the health of the Eden service
    #[structopt(alias = "health")]
    Status(crate::status::StatusCmd),

    /// Print the daemon's process ID if running
    Pid(crate::pid::PidCmd),
}

impl Command {
    fn get_etc_eden_dir(&self) -> PathBuf {
        if let Some(etc_eden_dir) = &self.etc_eden_dir {
            etc_eden_dir.clone()
        } else {
            DEFAULT_ETC_EDEN_DIR.into()
        }
    }

    fn get_config_dir(&self) -> PathBuf {
        if let Some(config_dir) = &self.config_dir {
            config_dir.clone()
        } else {
            expand_path(DEFAULT_CONFIG_DIR)
        }
    }

    fn get_home_dir(&self) -> Option<PathBuf> {
        if let Some(home_dir) = &self.home_dir {
            Some(home_dir.clone())
        } else {
            dirs::home_dir()
        }
    }

    fn get_instance(&self) -> EdenFsInstance {
        EdenFsInstance::new(
            self.get_config_dir(),
            self.get_etc_eden_dir(),
            self.get_home_dir(),
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
        event!(Level::TRACE, cmd = ?self, "Dispatching");

        let instance = self.get_instance();
        match self.subcommand {
            SubCommand::Status(status) => status.run(instance).await,
            SubCommand::Pid(pid_cmd) => pid_cmd.run(instance).await,
        }
    }
}
