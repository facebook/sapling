/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use hg_util::path::expand_path;
use tracing::event;
use tracing::Level;

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
mod util;

#[cfg(unix)]
const DEFAULT_CONFIG_DIR: &str = "~/local/.eden";
#[cfg(unix)]
const DEFAULT_ETC_EDEN_DIR: &str = "/etc/eden";

#[cfg(windows)]
const DEFAULT_CONFIG_DIR: &str = "~\\.eden";
#[cfg(windows)]
const DEFAULT_ETC_EDEN_DIR: &str = "C:\\ProgramData\\facebook\\eden";

// Used to determine whether we should gate off certain oxidized edenfsctl commands
const ROLLOUT_JSON: &str = "edenfsctl_rollout.json";
const EXPERIMENTAL_COMMANDS: &[&str] = &["redirect"];

type ExitCode = i32;

#[derive(Parser, Debug)]
#[clap(
    name = "edenfsctl",
    disable_version_flag = true,
    disable_help_flag = true
)]
pub struct MainCommand {
    /// The path to the directory where edenfs stores its internal state.
    #[clap(global = true, long, parse(from_str = expand_path))]
    config_dir: Option<PathBuf>,

    /// Path to directory that holds the system configuration files.
    #[clap(global = true, long, parse(from_str = expand_path))]
    etc_eden_dir: Option<PathBuf>,

    /// Path to directory where .edenrc config file is stored.
    #[clap(global = true, long, parse(from_str = expand_path))]
    home_dir: Option<PathBuf>,

    /// Path to directory within a checkout.
    #[clap(global = true, long, parse(from_str = expand_path), hide = true)]
    checkout_dir: Option<PathBuf>,

    #[clap(global = true, long)]
    pub debug: bool,

    #[clap(subcommand)]
    pub subcommand: TopLevelSubcommand,
}

/// The first level of edenfsctl subcommands.
#[async_trait]
pub trait Subcommand: Send + Sync {
    async fn run(&self) -> Result<ExitCode>;

    fn get_mount_path_override(&self) -> Option<PathBuf> {
        None
    }
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
    #[clap(subcommand, alias = "redir")]
    Redirect(crate::redirect::RedirectCmd),
}

impl TopLevelSubcommand {
    fn subcommand(&self) -> &dyn Subcommand {
        use TopLevelSubcommand::*;

        match self {
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
            Redirect(cmd) => cmd,
        }
    }
}

#[async_trait]
impl Subcommand for TopLevelSubcommand {
    async fn run(&self) -> Result<ExitCode> {
        self.subcommand().run().await
    }

    fn get_mount_path_override(&self) -> Option<PathBuf> {
        self.subcommand().get_mount_path_override()
    }
}

impl fmt::Display for TopLevelSubcommand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                TopLevelSubcommand::Status(_) => "status",
                TopLevelSubcommand::Pid(_) => "pid",
                TopLevelSubcommand::Uptime(_) => "uptime",
                //TopLevelSubcommand::Gc(_) => "gc",
                TopLevelSubcommand::Config(_) => "config",
                TopLevelSubcommand::Debug(_) => "debug",
                //TopLevelSubcommand::Top(_) => "top",
                TopLevelSubcommand::Minitop(_) => "minitop",
                TopLevelSubcommand::Du(_) => "du",
                TopLevelSubcommand::List(_) => "list",
                TopLevelSubcommand::PrefetchProfile(_) => "prefetch-profile",
                TopLevelSubcommand::Redirect(_) => "redirect",
            }
        )
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
        // A config dir might be provided as a top-level argument. Top-level arguments take
        // precedent over sub-command args.
        if let Some(config_dir) = &self.config_dir {
            config_dir.clone()
        // Then check if the optional mount path provided by some subcommands is an EdenFS mount.
        // If it's provided and is a valid EdenFS mount, use the mounts config dir.
        } else if let Some(config_dir) = self
            .subcommand
            .get_mount_path_override()
            .and_then(|x| util::locate_eden_config_dir(&x))
        {
            config_dir
        // Then check if the current working directory is an EdenFS mount. If not, we should
        // default to the default config-dir location which varies by platform.
        } else {
            env::current_dir()
                .map_err(From::from)
                .and_then(|cwd| {
                    util::locate_eden_config_dir(&cwd)
                        .ok_or_else(|| anyhow!("cwd is not in an eden mount"))
                })
                .unwrap_or(expand_path(DEFAULT_CONFIG_DIR))
        }
    }

    fn get_home_dir(&self) -> Option<PathBuf> {
        if let Some(home_dir) = &self.home_dir {
            Some(home_dir.clone())
        } else {
            dirs::home_dir()
        }
    }

    fn set_working_directory(&self) -> Result<()> {
        if let Some(checkout_dir) = &self.checkout_dir {
            std::env::set_current_dir(checkout_dir).with_context(|| {
                format!(
                    "Unable to change to checkout directory: {}",
                    checkout_dir.display()
                )
            })?;
        }
        Ok(())
    }

    pub fn is_enabled_in_json(&self, name: &str) -> Option<bool> {
        let rollout_json_path = self.get_etc_eden_dir().join(ROLLOUT_JSON);
        if !rollout_json_path.exists() {
            return None;
        }

        // Open the file in read-only mode with buffer.
        let file = File::open(rollout_json_path).ok()?;
        let reader = BufReader::new(file);
        let json: serde_json::Value = serde_json::from_reader(reader).ok()?;
        let map = json.as_object()?;

        map.get(name).and_then(|v| v.as_bool())
    }

    /// For experimental commands, we should check whether Chef enabled the command for our shard. If not, fall back to python cli
    pub fn is_enabled(&self) -> bool {
        let name = self.subcommand.to_string();
        self.is_enabled_in_json(&name)
            .unwrap_or_else(|| !EXPERIMENTAL_COMMANDS.contains(&name.as_ref()))
    }
    pub fn run(self) -> Result<ExitCode> {
        self.set_working_directory()?;

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

        EdenFsInstance::init(
            self.get_config_dir(),
            self.get_etc_eden_dir(),
            self.get_home_dir(),
        );
        // Use EdenFsInstance::global() to access the instance from now on
        self.subcommand.run().await
    }
}
