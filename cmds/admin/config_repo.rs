// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::Arc;

use clap::{App, Arg, ArgMatches, SubCommand};
use failure::{Error, Result};
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use promptly::Promptable;
use slog::Logger;
use tokio_process::CommandExt;

use blobrepo::BlobRepo;
use cmdlib::{args::setup_blobrepo_dir, blobimport_lib::Blobimport};
use mercurial_types::RepositoryId;

const CLONE_CMD: &'static str = "clone";
const CLONE_DFLT_DIR: &'static str = "mononoke-config";
const IMPORT_CMD: &'static str = "import";
const IMPORT_DFLT_DIR: &'static str = "mononoke-config-imported";

const HGRC_CONTENT: &'static str = "
[extensions]
treemanifest=
fastmanifest=!

[treemanifest]
treeonly=True
server=True
";

#[derive(Debug, Fail)]
enum ErrorKind {
    #[fail(display = "Aborting: path {:#?} exists, but is not a directory", _0)]
    ExpectedDir(PathBuf),
    #[fail(display = "Aborting: directory {:#?} exists, but is not empty", _0)] NotEmpty(PathBuf),
    #[fail(display = "Aborting: command '{}' killed by a signal", _0)] KilledBySignal(&'static str),
    #[fail(display = "Aborting: command '{}' exited with exit status {}", _0, _1)]
    NonZeroExit(&'static str, i32),
}

pub fn prepare_command<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    let clone = SubCommand::with_name(CLONE_CMD)
        .about("clone the mononoke-config repository")
        .add_interactive()
        .add_dest();

    let import = SubCommand::with_name(IMPORT_CMD)
        .about("import the mononoke-config into rocksdb blobrepo")
        .add_interactive()
        .add_dest()
        .add_src();

    app.about("set of commands to interact with mononoke-config repository")
        .subcommand(clone)
        .subcommand(import)
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

pub fn handle_command<'a>(matches: &ArgMatches<'a>, logger: Logger) -> BoxFuture<(), Error> {
    match matches.subcommand() {
        (CLONE_CMD, Some(sub_m)) => handle_clone(sub_m, logger),
        (IMPORT_CMD, Some(sub_m)) => handle_import(sub_m, logger),
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

fn handle_clone<'a>(args: &ArgMatches<'a>, logger: Logger) -> BoxFuture<(), Error> {
    let interactive = args.is_present("interactive");
    let dest = {
        let default = try_boxfuture!(data_dir()).join(CLONE_DFLT_DIR);

        match args.value_of("dest") {
            Some(dir) => PathBuf::from(dir),
            None if interactive => {
                PathBuf::prompt_default("Specify destination folder for cloning", default)
            }
            None => default,
        }
    };

    info!(
        logger,
        "Using {} as destination for cloning",
        dest.display()
    );

    try_boxfuture!(remove_dir(dest.clone(), interactive));

    clone(dest)
}

fn handle_import<'a>(args: &ArgMatches<'a>, logger: Logger) -> BoxFuture<(), Error> {
    let interactive = args.is_present("interactive");
    let dest = {
        let default = try_boxfuture!(data_dir()).join(IMPORT_DFLT_DIR);

        match args.value_of("dest") {
            Some(dir) => PathBuf::from(dir),
            None if interactive => {
                PathBuf::prompt_default("Specify destination folder for importing", default)
            }
            None => default,
        }
    };

    info!(
        logger,
        "Using {} as destination for importing",
        dest.display()
    );

    try_boxfuture!(remove_dir(dest.clone(), interactive));

    let src = {
        let default = try_boxfuture!(data_dir()).join(CLONE_DFLT_DIR);

        match args.value_of("src") {
            Some(dir) => PathBuf::from(dir),
            None if interactive => {
                PathBuf::prompt_default("Specify src folder for importing", default)
            }
            None => default,
        }
    };

    info!(logger, "Using {} as source for importing", src.display());

    try_boxfuture!(fs::create_dir_all(&dest));
    import(logger, src, dest)
}

fn data_dir() -> Result<PathBuf> {
    Ok(PathBuf::from("/data/users").join(env::var("USER")?))
}

fn remove_dir(dir: PathBuf, interactive: bool) -> Result<()> {
    ensure_err!(!dir.exists() || dir.is_dir(), ErrorKind::ExpectedDir(dir));

    if dir.exists() {
        if fs::read_dir(&dir)?.count() > 0 {
            ensure_err!(
                interactive
                    && bool::prompt_default(
                        format!("{} is not empty, remove it's content?", dir.display()),
                        false
                    ),
                ErrorKind::NotEmpty(dir)
            );
        }

        fs::remove_dir_all(&dir)?;
    }

    Ok(())
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

/// Assumes that the "dest" is a path to an empty directory
fn clone(dest: PathBuf) -> BoxFuture<(), Error> {
    Command::new("hg")
        .arg("clone")
        .arg("ssh://hg.vip.facebook.com//data/scm/mononoke-config")
        .arg(&dest)
        .status_async()
        .into_future()
        .flatten()
        .from_err()
        .and_then(|status| check_status(status, "hg clone"))
        .and_then(move |()| {
            let mut hgrc_file = fs::OpenOptions::new()
                .append(true)
                .open(dest.join(".hg/hgrc"))?;
            hgrc_file.write_all(HGRC_CONTENT.as_bytes())?;
            Ok(())
        })
        .boxify()
}

fn import(logger: Logger, src: PathBuf, dest: PathBuf) -> BoxFuture<(), Error> {
    try_boxfuture!(setup_blobrepo_dir(&dest, true));
    let blobrepo = Arc::new(try_boxfuture!(BlobRepo::new_rocksdb(
        logger.new(o!["BlobRepo:Rocksdb" => dest.to_string_lossy().into_owned()]),
        &dest,
        RepositoryId::new(0),
    )));

    Blobimport {
        logger,
        blobrepo,
        revlogrepo_path: src.join(".hg"),
        changeset: None,
        skip: None,
        commits_limit: None,
        no_bookmark: false,
    }.import()
}
