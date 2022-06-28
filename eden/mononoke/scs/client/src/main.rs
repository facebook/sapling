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
use clap::App;
use clap::AppSettings;
use fbinit::FacebookInit;

mod args;
mod commands;
mod connection;
mod lib;
mod render;
mod util;

#[cfg(not(target_os = "windows"))]
fn versions() -> (String, String) {
    use build_info::BuildInfo;

    let short_version = format!(
        "{}-{}",
        BuildInfo::get_package_version(),
        BuildInfo::get_package_release(),
    );
    let long_version = format!("{:#?}", BuildInfo);
    (short_version, long_version)
}

#[cfg(target_os = "windows")]
fn versions() -> (String, String) {
    let short_version = String::from("for Windows");
    let long_version = String::from("(BuildInfo not available on Windows)");
    (short_version, long_version)
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> ExitCode {
    let (short_version, long_version) = versions();
    let mut app = App::new("Source Control Service client")
        .version(short_version.as_ref())
        .long_version(long_version.as_ref())
        .about("Send requests to the Source Control Service")
        .set_term_width(textwrap::termwidth())
        .setting(AppSettings::ColoredHelp)
        .setting(AppSettings::SubcommandRequired);
    app = connection::add_args(app);
    app = commands::add_args(app);
    if let Err(e) = commands::dispatch(fb, app.get_matches()).await {
        let prog_name = env::args().next().unwrap_or("scsc".to_string());
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
