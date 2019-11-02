/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod error;

use self::error::*;
use clap::{App, Arg};
use commitcloudsubscriber::{
    CommitCloudConfig, CommitCloudTcpReceiverService, CommitCloudWorkspaceSubscriberService,
};
use failure::{bail, Fallible};
use log::info;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;

#[cfg(target_os = "macos")]
use std::io::Write;

/// This is what we're going to decode toml config into.
/// Each field is optional, meaning that it doesn't have to be present in TOML.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub title: Option<String>,
    /// [commitcloud] section: commitcloudlib provides description of it
    pub commitcloud: Option<CommitCloudConfig>,
}

// To support older than Rust 1.26 on dev servers
fn main() {
    run().unwrap();
}

fn run() -> Fallible<()> {
    check_nice()?;
    env_logger::init();
    let help: &str = &format!(
        "{}\n{}",
        "The SCM Daemon is a program to speed up and facilitate mercurial commands and extensions",
        "The SCM Daemon runs as a service, logging its operations directly into stdout, \
         and init systems like systemd or launchd will automatically handle everything else, \
         including startup, shutdown, logging redirection, lifecycle management etc.",
    );

    let matches = App::new("SCM Daemon")
        .version("1.0.0")
        .help(help)
        .args(&[
            Arg::from_usage("--config [config file (toml format)]").required(true),
            Arg::from_usage("--pidfile [specify path to pidfile]").required(false),
        ])
        .get_matches();

    // write pidfile
    // do not rely on existence of this file to check if program running
    // std::process::id unstable feature for old compiler
    // so add #[cfg(target_os = "macos")] temporary
    #[cfg(target_os = "macos")]
    {
        if let Some(path) = matches.value_of("pidfile") {
            File::create(path)?.write_fmt(format_args!("{}", std::process::id()))?;
        }
    }

    // read required config path
    let configfile = matches.value_of("config").unwrap();

    info!("Reading Scm Daemon configuration from {}", configfile);

    // parse the toml config
    let config: Config = toml::from_str(&{
        let mut f = File::open(configfile)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        content
    })?;

    // commit cloud part of the configuration
    let commitcloudconfref = &config
        .commitcloud
        .unwrap_or_else(|| toml::from_str::<CommitCloudConfig>("").unwrap());

    let commitcloud_workspacesubscriber =
        CommitCloudWorkspaceSubscriberService::new(commitcloudconfref)?;
    let commitcloud_tcpreceiver =
        CommitCloudTcpReceiverService::new(commitcloudconfref.tcp_receiver_port)
            .with_actions(commitcloud_workspacesubscriber.actions());

    // start services
    let commitcloud_tcpreceiver_handler = commitcloud_tcpreceiver.serve()?;
    let commitcloud_workspacesubscriber_handler = commitcloud_workspacesubscriber.serve()?;

    // join running services, this will block
    match commitcloud_tcpreceiver_handler.join() {
        Ok(result) => result?,
        Err(_) => bail!("commitcloud tcpreceiver panicked"),
    };

    match commitcloud_workspacesubscriber_handler.join() {
        Ok(result) => result?,
        Err(_) => bail!("commitcloud workspace subscriber panicked"),
    };

    Ok(())
}

/// Refuse to run if nice is too high (i.e. process has low priority)
///
/// This is because the hg processes spawned by this daemon inherit the
/// priority, and they can be quite slow if run on low priority.
fn check_nice() -> Fallible<()> {
    #[cfg(unix)]
    {
        let nice = unsafe { libc::nice(0) };
        if nice > 0 {
            bail!("refuse to run on low priority (nice = {})", nice)
        }
    }

    Ok(())
}
