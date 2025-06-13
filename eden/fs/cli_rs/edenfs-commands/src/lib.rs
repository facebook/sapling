/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(impl_trait_in_fn_trait_return)]

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::instance::EdenFsInstance;
use edenfs_client::use_case::UseCaseId;
use edenfs_client::utils::get_config_dir;
use edenfs_client::utils::get_etc_eden_dir;
use edenfs_client::utils::get_home_dir;
use hg_util::path::expand_path;
use tracing::Level;
use tracing::event;

mod config;
mod debug;
mod du;
#[cfg(target_os = "macos")]
mod file_access_monitor;
mod gc;
mod glob_and_prefetch;
mod handles;
mod list;
mod minitop;
mod notify;
mod pid;
mod prefetch_profile;
mod redirect;
mod remove;
mod socket;
mod status;
mod top;
mod uptime;
mod util;

// Used to determine whether we should gate off certain oxidized edenfsctl commands
const ROLLOUT_JSON: &str = "edenfsctl_rollout.json";
const EXPERIMENTAL_COMMANDS: &[&str] = &["glob", "prefetch", "remove"];

// We create a single EdenFsInstance when starting up
static EDENFS_INSTANCE: OnceLock<EdenFsInstance> = OnceLock::new();

pub(crate) fn get_edenfs_instance() -> &'static EdenFsInstance {
    EDENFS_INSTANCE
        .get()
        .expect("EdenFsInstance is not initialized")
}

fn init_edenfs_instance(config_dir: PathBuf, etc_eden_dir: PathBuf, home_dir: Option<PathBuf>) {
    event!(
        Level::TRACE,
        ?config_dir,
        ?etc_eden_dir,
        ?home_dir,
        "Creating EdenFsInstance"
    );
    EDENFS_INSTANCE
        .set(EdenFsInstance::new(
            UseCaseId::EdenFsCtl,
            config_dir,
            etc_eden_dir,
            home_dir,
        ))
        .expect("should be able to initialize EdenfsInstance")
}

type ExitCode = i32;

#[derive(Parser, Debug)]
#[clap(
    name = "edenfsctl",
    disable_version_flag = true,
    disable_help_flag = false,
    next_help_heading = "GLOBAL OPTIONS"
)]
pub struct MainCommand {
    /// Path to directory where edenfs stores its internal state
    #[clap(global = true, long, parse(from_str = expand_path))]
    config_dir: Option<PathBuf>,

    /// Path to directory that holds the system configuration files
    #[clap(global = true, long, parse(from_str = expand_path))]
    etc_eden_dir: Option<PathBuf>,

    /// Path to directory where .edenrc config file is stored
    #[clap(global = true, long, parse(from_str = expand_path))]
    home_dir: Option<PathBuf>,

    /// Path to directory within a checkout
    #[clap(global = true, long, parse(from_str = expand_path), hide = true)]
    checkout_dir: Option<PathBuf>,

    /// Enable debug mode (more verbose logging, traceback, etc..)
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
    Config(crate::config::CliConfigCmd),
    Debug(crate::debug::DebugCmd),
    Du(crate::du::DiskUsageCmd),
    Fsconfig(crate::config::FsConfigCmd),
    List(crate::list::ListCmd),
    Minitop(crate::minitop::MinitopCmd),
    #[clap(alias = "notification")]
    Notify(crate::notify::NotifyCmd),
    Pid(crate::pid::PidCmd),
    #[clap(subcommand, alias = "pp")]
    PrefetchProfile(crate::prefetch_profile::PrefetchProfileCmd),
    #[clap(subcommand, alias = "redir")]
    Redirect(crate::redirect::RedirectCmd),
    #[clap(alias = "rm")]
    Remove(crate::remove::RemoveCmd),
    #[cfg(target_os = "windows")]
    Handles(crate::handles::HandlesCmd),
    Reloadconfig(crate::config::ReloadConfigCmd),
    #[clap(alias = "sock")]
    Socket(crate::socket::SocketCmd),
    #[clap(alias = "health")]
    Status(crate::status::StatusCmd),
    // Top(crate::top::TopCmd),
    Uptime(crate::uptime::UptimeCmd),
    #[cfg(target_os = "macos")]
    FileAccessMonitor(crate::file_access_monitor::FileAccessMonitorCmd),
    Glob(crate::glob_and_prefetch::GlobCmd),
    Prefetch(crate::glob_and_prefetch::PrefetchCmd),
}

impl TopLevelSubcommand {
    fn subcommand(&self) -> &dyn Subcommand {
        use TopLevelSubcommand::*;

        match self {
            Config(cmd) => cmd,
            Reloadconfig(cmd) => cmd,
            Fsconfig(cmd) => cmd,
            Debug(cmd) => cmd,
            Du(cmd) => cmd,
            // Gc(cmd) => cmd,
            List(cmd) => cmd,
            Minitop(cmd) => cmd,
            Notify(cmd) => cmd,
            Pid(cmd) => cmd,
            PrefetchProfile(cmd) => cmd,
            Redirect(cmd) => cmd,
            Remove(cmd) => cmd,
            #[cfg(target_os = "windows")]
            Handles(cmd) => cmd,
            Socket(cmd) => cmd,
            Status(cmd) => cmd,
            // Top(cmd) => cmd,
            Uptime(cmd) => cmd,
            #[cfg(target_os = "macos")]
            FileAccessMonitor(cmd) => cmd,
            Glob(cmd) => cmd,
            Prefetch(cmd) => cmd,
        }
    }

    pub fn name(&self) -> &'static str {
        // TODO: Is there a way to extract the subcommand's name from clap?
        // Otherwise, there is a risk of divergence with clap's own attributes.
        match self {
            TopLevelSubcommand::Config(_) => "config",
            TopLevelSubcommand::Debug(_) => "debug",
            TopLevelSubcommand::Du(_) => "du",
            TopLevelSubcommand::Fsconfig(_) => "fsconfig",
            // TopLevelSubcommand::Gc(_) => "gc",
            #[cfg(target_os = "windows")]
            TopLevelSubcommand::Handles(_) => "handles",
            TopLevelSubcommand::List(_) => "list",
            TopLevelSubcommand::Minitop(_) => "minitop",
            TopLevelSubcommand::Notify(_) => "notify",
            TopLevelSubcommand::Pid(_) => "pid",
            TopLevelSubcommand::PrefetchProfile(_) => "prefetch-profile",
            TopLevelSubcommand::Redirect(_) => "redirect",
            TopLevelSubcommand::Remove(_) => "remove",
            TopLevelSubcommand::Reloadconfig(_) => "reloadconfig",
            TopLevelSubcommand::Socket(_) => "socket",
            TopLevelSubcommand::Status(_) => "status",
            //TopLevelSubcommand::Top(_) => "top",
            TopLevelSubcommand::Uptime(_) => "uptime",
            #[cfg(target_os = "macos")]
            TopLevelSubcommand::FileAccessMonitor(_) => "file-access-monitor",
            TopLevelSubcommand::Glob(_) => "glob",
            TopLevelSubcommand::Prefetch(_) => "prefetch",
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

impl MainCommand {
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

    /// For experimental commands, we should check whether Chef enabled the command for our shard. If not, fall back to python cli
    pub fn is_enabled(&self) -> bool {
        is_command_enabled_in_rust(self.subcommand.name(), &self.etc_eden_dir, &None)
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

        init_edenfs_instance(
            get_config_dir(&self.config_dir, &self.subcommand.get_mount_path_override())?,
            get_etc_eden_dir(&self.etc_eden_dir),
            get_home_dir(&self.home_dir),
        );
        // Use get_edenfs_instance() to access the instance from now on
        self.subcommand.run().await
    }
}

pub fn is_command_enabled_in_rust(
    name: &str,
    etc_eden_dir_override: &Option<PathBuf>,
    experimental_commands_override: &Option<Vec<&str>>,
) -> bool {
    is_command_enabled_in_json(name, etc_eden_dir_override).unwrap_or_else(|| {
        let effective_exp_commands = match experimental_commands_override {
            Some(vec) => vec,
            None => EXPERIMENTAL_COMMANDS,
        };
        !effective_exp_commands.contains(&name)
    })
}

fn is_command_enabled_in_json(name: &str, etc_eden_dir_override: &Option<PathBuf>) -> Option<bool> {
    let rollout_json_path = get_etc_eden_dir(etc_eden_dir_override).join(ROLLOUT_JSON);
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
