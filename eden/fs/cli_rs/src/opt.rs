/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use structopt::{clap::AppSettings, StructOpt};

/// Manage EdenFS checkouts
#[derive(StructOpt, Debug)]
#[structopt(
    name = "edenfsctl",
    setting = AppSettings::DisableVersion,
    setting = AppSettings::DisableHelpFlags
)]
pub struct Opt {
    /// The path to the directory where edenfs stores its internal state.
    #[structopt(long)]
    config_dir: Option<PathBuf>,

    /// Path to directory that holds the system configuration files.
    #[structopt(long)]
    ect_eden_dir: Option<PathBuf>,

    /// Path to directory where .edenrc config file is stored.
    #[structopt(long)]
    home_dir: Option<PathBuf>,

    #[structopt(subcommand)]
    subcommand: SubCommand,
}

#[derive(StructOpt, Debug)]
pub enum SubCommand {
    /// Check the health of the Eden service
    #[structopt()]
    Status {
        /// Wait up to TIMEOUT seconds for the daemon to respond
        #[structopt(default_value = "3")]
        timeout: i32,
    },
}
