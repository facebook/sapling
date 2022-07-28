/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Command-line client for the Source Control Service.

use std::env;
use std::process::ExitCode;

use ansi_term::Colour;
use atty::Stream;
use base_app::BaseApp;
use clap::ArgMatches;
use clap::CommandFactory;
use clap::FromArgMatches;
use clap::Parser;
use fbinit::FacebookInit;

use crate::connection::Connection;
use crate::connection::ConnectionArgs;
use crate::render::OutputTarget;

mod args;
mod commands;
mod connection;
pub(crate) mod lib;
mod render;
pub(crate) mod util;

lazy_static::lazy_static! {
    static ref SHORT_VERSION: String = {
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
    };
    static ref LONG_VERSION: String = {
        #[cfg(target_os = "windows")]
        {
            String::from("(BuildInfo not available on Windows)")
        }
        #[cfg(not(target_os = "windows"))]
        {
            use build_info::BuildInfo;
            format!("{:#?}", BuildInfo)
        }
    };
}

pub(crate) struct ScscApp {
    matches: ArgMatches,
    connection: Connection,
    target: OutputTarget,
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
struct SCSCArgs {
    /// Should the output of the command be JSON?
    #[clap(long)]
    json: bool,

    #[clap(flatten)]
    connection_args: ConnectionArgs,
}

async fn main_impl(fb: FacebookInit) -> anyhow::Result<()> {
    let subcommands = commands::subcommands();
    assert!(!subcommands.is_empty());
    let app = SCSCArgs::command()
        .subcommands(subcommands)
        .subcommand_required(true)
        .arg_required_else_help(true);
    let matches = app.get_matches();
    let common_args = SCSCArgs::from_arg_matches(&matches)?;
    let connection = common_args.connection_args.get_connection(fb)?;
    let target = if common_args.json {
        OutputTarget::Json
    } else if atty::is(atty::Stream::Stdout) {
        OutputTarget::Tty
    } else {
        OutputTarget::Pipe
    };
    let app = ScscApp {
        matches,
        connection,
        target,
    };
    commands::dispatch(app).await
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> ExitCode {
    if let Err(e) = main_impl(fb).await {
        let prog_name = env::args().next().unwrap_or_else(|| "scsc".to_string());
        if atty::is(Stream::Stderr) {
            eprintln!(
                "{}: {} {}",
                prog_name,
                Colour::Red.bold().paint("error:"),
                e
            );
        } else {
            eprintln!("{}: error: {}", prog_name, e);
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
