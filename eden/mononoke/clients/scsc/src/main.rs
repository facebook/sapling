/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

//! Command-line client for the Source Control Service.

// The test-support build of the scmquery client is a separate crate
// (`scmqueryclient_rust_test_support`) so it can coexist with the prod crate in
// one build graph without a name collision; alias it back to the canonical
// `scmqueryclient_rust::` path so submodules don't care which is linked.
extern crate scmqueryclient_rust_test_support as scmqueryclient_rust;

use std::env;
use std::io::IsTerminal;
use std::io::stderr;
use std::process::ExitCode;
use std::sync::LazyLock;

use ansi_term::Colour;
use base_app::BaseApp;
use clap::ArgMatches;
use clap::CommandFactory;
use clap::FromArgMatches;
use clap::Parser;
use fbinit::FacebookInit;
use scs_client_raw::ScsClient;

use crate::connection::ConnectionArgs;
use crate::render::OutputFormat;

mod args;
mod commands;
mod connection;
mod errors;
pub(crate) mod library;
mod render;
pub(crate) mod util;

const SCSC_ADMIN_ENABLED_ENV: &str = "SCSC_ADMIN_ENABLED";
const SCSC_PRINT_CORRELATOR_ENV: &str = "SCSC_PRINT_CORRELATOR";

static SHORT_VERSION: LazyLock<String> = LazyLock::new(|| {
    #[cfg(target_os = "windows")]
    {
        String::from("for Windows")
    }
    #[cfg(not(target_os = "windows"))]
    {
        use build_info::BuildInfo;
        format!(
            "{}-{}",
            BuildInfo::get_package_version(),
            BuildInfo::get_package_release(),
        )
    }
});

static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    #[cfg(target_os = "windows")]
    {
        String::from("(BuildInfo not available on Windows)")
    }
    #[cfg(not(target_os = "windows"))]
    {
        use build_info::BuildInfo;
        format!("{BuildInfo:#?}")
    }
});

pub(crate) struct ScscApp {
    matches: ArgMatches,
    connection_args: ConnectionArgs,
    print_correlator: bool,
    target: OutputFormat,
    fb: FacebookInit,
}

impl ScscApp {
    pub(crate) fn scs_host(&self) -> Option<&str> {
        self.connection_args.host()
    }

    async fn get_connection(&self, repo: Option<&str>) -> anyhow::Result<ScsClient> {
        let conn = self.connection_args.get_connection(self.fb, repo).await?;
        if self.print_correlator {
            match conn.get_client_corrrelator() {
                Some(correlator) => println!("Client correlator: {correlator}"),
                None => println!("Client correlator: <none>"),
            }
        }
        Ok(conn)
    }
}

impl BaseApp for ScscApp {
    fn subcommand(&self) -> Option<(&str, &ArgMatches)> {
        self.matches.subcommand()
    }
}

#[derive(Parser)]
#[clap(
    name = "Source Control Service client",
    version(&**SHORT_VERSION),
    long_version(&**LONG_VERSION),
    term_width(textwrap::termwidth()),
)]
/// Send requests to the Source Control Service
struct ScscArgs {
    /// Should the output of the command be JSON?
    #[clap(long, global = true)]
    json: bool,

    /// Path to a JustKnobs override config (JSON) to load via cached_config.
    /// Hidden: used by integration tests to flip knobs (e.g.
    /// `scm/scmquery:direct_scs`) through `merge_just_knobs`.
    #[clap(long, hide = true)]
    just_knobs_config_path: Option<String>,

    #[clap(flatten)]
    connection_args: ConnectionArgs,
}

async fn init_just_knobs_from_config_path(path: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let config_json = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading just-knobs config from {path}"))?;
    let config_handle = cached_config::ConfigHandle::from_json(&config_json)
        .context("parsing just-knobs config")?;
    justknobs::init_cached_config_just_knobs(&(), &config_handle)
        .context("initializing cached_config JustKnobs")
}

async fn main_impl(fb: FacebookInit) -> anyhow::Result<()> {
    if hostcaps::is_corp() {
        //In Corp we should not be using strict mode of fbwhoami, which throws an error if the file is not present.
        gflags::set_gflag_value(fb, "fbwhoami_strict", gflags::GflagValue::Bool(false))?
    }
    let subcommands = commands::subcommands();
    assert!(!subcommands.is_empty());
    let scsc_admin_enabled = env::var_os(SCSC_ADMIN_ENABLED_ENV).is_some();
    let app = ScscArgs::command()
        .subcommands(subcommands)
        .subcommand_required(true)
        .arg_required_else_help(true);
    let matches = app.get_matches();
    let common_args = ScscArgs::from_arg_matches(&matches)?;
    if let Some(just_knobs_config_path) = &common_args.just_knobs_config_path {
        init_just_knobs_from_config_path(just_knobs_config_path).await?;
    }
    let connection_args = common_args.connection_args;
    let target = if common_args.json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };
    let print_correlator = scsc_admin_enabled && env::var_os(SCSC_PRINT_CORRELATOR_ENV).is_some();
    let app = ScscApp {
        matches,
        connection_args,
        print_correlator,
        target,
        fb,
    };
    commands::dispatch(app).await
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> ExitCode {
    cpp_log_spew::disable(fb);

    if let Err(e) = main_impl(fb).await {
        let prog_name = env::args().next().unwrap_or_else(|| "scsc".to_string());
        if stderr().is_terminal() {
            eprintln!(
                "{}: {} {:#}",
                prog_name,
                Colour::Red.bold().paint("error:"),
                e
            );
        } else {
            eprintln!("{prog_name}: error: {e:#}");
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
