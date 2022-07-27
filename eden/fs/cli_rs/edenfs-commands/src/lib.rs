/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use tracing::event;
use tracing::Level;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;
use util::path::expand_path;

mod config;
mod debug;
mod du;
mod gc;
mod list;
mod minitop;
mod pid;
mod prefetch_profile;
mod redirect;
mod status;
mod top;
mod uptime;

#[cfg(unix)]
const DEFAULT_CONFIG_DIR: &str = "~/local/.eden";
#[cfg(unix)]
const DEFAULT_ETC_EDEN_DIR: &str = "/etc/eden";

#[cfg(windows)]
const DEFAULT_CONFIG_DIR: &str = "~\\.eden";
#[cfg(windows)]
const DEFAULT_ETC_EDEN_DIR: &str = "C:\\ProgramData\\facebook\\eden";

type ExitCode = i32;

#[derive(Parser, Debug)]
#[clap(
    name = "edenfsctl",
    disable_version_flag = true,
    disable_help_flag = true
)]
pub struct MainCommand {
    /// The path to the directory where edenfs stores its internal state.
    #[clap(long, parse(from_str = expand_path))]
    config_dir: Option<PathBuf>,

    /// Path to directory that holds the system configuration files.
    #[clap(long, parse(from_str = expand_path))]
    etc_eden_dir: Option<PathBuf>,

    /// Path to directory where .edenrc config file is stored.
    #[clap(long, parse(from_str = expand_path))]
    home_dir: Option<PathBuf>,

    #[clap(long)]
    pub debug: bool,

    #[clap(subcommand)]
    subcommand: TopLevelSubcommand,
}

/// The first level of edenfsctl subcommands.
#[async_trait]
pub trait Subcommand: Send + Sync {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode>;
}

/**
 * The first level of edenfsctl subcommands.
 */
#[derive(Parser, Debug)]
pub enum TopLevelSubcommand {
    #[clap(alias = "health")]
    Status(crate::status::StatusCmd),
    Pid(crate::pid::PidCmd),
    Uptime(crate::uptime::UptimeCmd),
    // Gc(crate::gc::GcCmd),
    Config(crate::config::ConfigCmd),
    Debug(crate::debug::DebugCmd),
    // Top(crate::top::TopCmd),
    Minitop(crate::minitop::MinitopCmd),
    Du(crate::du::DiskUsageCmd),
    List(crate::list::ListCmd),
    #[clap(subcommand, alias = "pp")]
    PrefetchProfile(crate::prefetch_profile::PrefetchCmd),
    // #[clap(subcommand, alias = "redir")]
    // Redirect(crate::redirect::RedirectCmd),
}

#[async_trait]
impl Subcommand for TopLevelSubcommand {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        use TopLevelSubcommand::*;
        let sc: &(dyn Subcommand) = match self {
            Status(cmd) => cmd,
            Pid(cmd) => cmd,
            Uptime(cmd) => cmd,
            // Gc(cmd) => cmd,
            Config(cmd) => cmd,
            Debug(cmd) => cmd,
            // Top(cmd) => cmd,
            Minitop(cmd) => cmd,
            Du(cmd) => cmd,
            List(cmd) => cmd,
            PrefetchProfile(cmd) => cmd,
            // Redirect(cmd) => cmd,
        };
        sc.run(instance).await
    }
}

impl MainCommand {
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

        runtime.block_on(self.dispatch())
    }

    /// Execute subcommands. This function returns only a return code since all the error handling
    /// should be taken care of by each sub-command.
    async fn dispatch(self) -> Result<ExitCode> {
        event!(Level::TRACE, cmd = ?self, "Dispatching");

        let instance = self.get_instance();
        self.subcommand.run(instance).await
    }
}
