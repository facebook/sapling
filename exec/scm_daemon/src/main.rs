extern crate clap;
extern crate commitcloudsubscriber;
extern crate env_logger;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
extern crate toml;

pub mod error;

use self::error::*;
use clap::{App, Arg};
use commitcloudsubscriber::{CommitCloudConfig, CommitCloudWorkspaceSubscriber};
use std::fs::File;
use std::io::Read;

/// This is what we're going to decode toml config into.
/// Each field is optional, meaning that it doesn't have to be present in TOML.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub title: Option<String>,
    /// [commitcloud] section: commitcloudlib provides description of it
    pub commitcloud: Option<CommitCloudConfig>,
}

fn main() -> Result<()> {
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
        ])
        .get_matches();

    // read required config path
    let configfile = matches.value_of("config").unwrap();

    info!("Reading Scm Daemon configuration from {}", configfile);

    // parse toml config
    let config: Config = toml::from_str(&{
        let mut f = File::open(configfile)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        content
    })?;

    {
        info!("[commitcloud] starting CommitCloudWorkspaceSubscriber");
        CommitCloudWorkspaceSubscriber::try_new(&config
            .commitcloud
            .unwrap_or_else(|| toml::from_str::<CommitCloudConfig>("").unwrap()))?
            .run()?;
    }
    Ok(())
}
