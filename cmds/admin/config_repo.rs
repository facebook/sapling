// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use clap::{App, Arg, ArgMatches, SubCommand};
use failure::{Error, Result};
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use promptly::Promptable;
use tokio_process::CommandExt;

const FBPKG_CMD: &'static str = "fbpkg";

#[derive(Debug, Fail)]
enum ErrorKind {
    #[fail(display = "Aborting: command '{}' killed by a signal", _0)] KilledBySignal(&'static str),
    #[fail(display = "Aborting: command '{}' exited with exit status {}", _0, _1)]
    NonZeroExit(&'static str, i32),
}

pub fn prepare_command<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    let fbpkg = SubCommand::with_name(FBPKG_CMD)
        .about("build fbpkg mononoke.config for tupperware deployments")
        .add_interactive()
        .arg(
            Arg::with_name("ephemeral")
                .long("ephemeral")
                .short("E")
                .takes_value(false)
                .help("Build an ephemeral package with fbpkg"),
        )
        .arg(
            Arg::with_name("non-forward")
                .long("non-forward")
                .short("f")
                .takes_value(false)
                .help("Do not use --revision-check on fbpkg build"),
        )
        .arg(
            Arg::with_name("src")
                .long("src")
                .short("s")
                .takes_value(true)
                .required(false)
                .help("Specify source folder"),
        );

    app.about("set of commands to interact with mononoke-config repository")
        .subcommand(fbpkg)
}

trait AppExt {
    fn add_interactive(self) -> Self;
    fn add_dest(self) -> Self;
    fn add_src(self) -> Self;
}

impl<'a, 'b> AppExt for App<'a, 'b> {
    fn add_interactive(self) -> Self {
        self.arg(
            Arg::with_name("interactive")
                .long("interactive")
                .short("i")
                .takes_value(false)
                .help(
                    "Turn on interactive prompt, makes it easier to use \
                     when defaults are not enough",
                ),
        )
    }

    fn add_dest(self) -> Self {
        self.arg(
            Arg::with_name("dest")
                .long("dest")
                .short("d")
                .takes_value(true)
                .required(false)
                .help("Specify destination folder"),
        )
    }

    fn add_src(self) -> Self {
        self.arg(
            Arg::with_name("src")
                .long("src")
                .short("s")
                .takes_value(true)
                .required(false)
                .help("Specify source folder"),
        )
    }
}

pub fn handle_command<'a>(matches: &ArgMatches<'a>) -> BoxFuture<(), Error> {
    match matches.subcommand() {
        (FBPKG_CMD, Some(sub_m)) => handle_fbpkg(sub_m),
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

fn handle_fbpkg<'a>(args: &ArgMatches<'a>) -> BoxFuture<(), Error> {
    let interactive = args.is_present("interactive");
    let ephemeral = args.is_present("ephemeral");
    let non_forward = args.is_present("non-forward");

    let src = {
        match args.value_of("src") {
            Some(dir) => PathBuf::from(dir),
            None if interactive => PathBuf::prompt("Specify src folder for importing"),
            None => panic!("Please specify source directory with config"),
        }
    };

    let mut fbpkg = Command::new("fbpkg");
    fbpkg
        .arg("build")
        .arg("mononoke.config")
        .arg("--set")
        .arg("import_dir")
        .arg(".");
    if !non_forward {
        fbpkg.arg("--revision-check");
    }
    if ephemeral {
        fbpkg.arg("--ephemeral");
    }
    fbpkg
        .current_dir(src)
        .status_async()
        .into_future()
        .flatten()
        .from_err()
        .and_then(|status| check_status(status, "fbpkg build"))
        .boxify()
}

fn check_status(status: ExitStatus, proc_name: &'static str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        match status.code() {
            None => Err(ErrorKind::KilledBySignal(proc_name).into()),
            Some(code) => Err(ErrorKind::NonZeroExit(proc_name, code).into()),
        }
    }
}
