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
use tempdir::TempDir;
use tokio_process::CommandExt;

use blobrepo::BlobRepo;
use cmdlib::{args::setup_repo_dir, blobimport_lib::Blobimport};
use mercurial_types::RepositoryId;

const CLONE_CMD: &'static str = "clone";
const CLONE_DFLT_DIR: &'static str = "mononoke-config";
const IMPORT_CMD: &'static str = "import";
const IMPORT_DFLT_DIR: &'static str = "mononoke-config-imported";
const FBPKG_CMD: &'static str = "fbpkg";

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
    #[fail(display = "Aborting: RC bookmark is not a descendant of PROD bookmark. Please move RC or PROD bookmarks")]
    RcNotDescendantOfProd(),
    #[fail(display = "Aborting: RC should be at most 1 commit ahead of PROD bookmark. Please move RC or PROD bookmarks")]
    RcTooFarAway(),
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

    let fbpkg = SubCommand::with_name(FBPKG_CMD)
        .about("build fbpkg mononoke.config for tupperware deployments")
        .add_interactive()
        .add_src()
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
        );

    app.about("set of commands to interact with mononoke-config repository")
        .subcommand(clone)
        .subcommand(import)
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

pub fn handle_command<'a>(matches: &ArgMatches<'a>, logger: Logger) -> BoxFuture<(), Error> {
    match matches.subcommand() {
        (CLONE_CMD, Some(sub_m)) => handle_clone(sub_m, logger),
        (IMPORT_CMD, Some(sub_m)) => handle_import(sub_m, logger),
        (FBPKG_CMD, Some(sub_m)) => handle_fbpkg(sub_m, logger),
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

fn handle_fbpkg<'a>(args: &ArgMatches<'a>, logger: Logger) -> BoxFuture<(), Error> {
    let interactive = args.is_present("interactive");
    let ephemeral = args.is_present("ephemeral");
    let non_forward = args.is_present("non-forward");

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

    let tmpdir = try_boxfuture!(TempDir::new(IMPORT_DFLT_DIR));
    // The imported content needs to be in a folder with deterministic name
    let import_dir = tmpdir.path().to_owned().join(IMPORT_DFLT_DIR);
    try_boxfuture!(fs::create_dir(&import_dir));

    set_local_bookmark(src.clone(), "PROD")
        .and_then({
            cloned!(src);
            move |()| set_local_bookmark(src.clone(), "RC")
        })
        .and_then({
            cloned!(src, import_dir);
            move |()| import(logger, src.clone(), import_dir)
        })
        .and_then(move |()| {
            let mut fbpkg = Command::new("fbpkg");
            fbpkg
                .arg("build")
                .arg("mononoke.config")
                .arg("--set")
                .arg("import_dir")
                .arg(import_dir);
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
        })
        // Make sure that the TempDir is dropped not earlier than at the end
        .map(move |()| { let _ = tmpdir; })
        .boxify()
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
    try_boxfuture!(setup_repo_dir(&dest, true));
    let blobrepo = Arc::new(try_boxfuture!(BlobRepo::new_rocksdb(
        logger.new(o!["BlobRepo:Rocksdb" => dest.to_string_lossy().into_owned()]),
        &dest,
        RepositoryId::new(0),
    )));

    check_hg_config_repo(src.clone())
        .and_then(move |()| {
            Blobimport {
                logger,
                blobrepo,
                revlogrepo_path: src.join(".hg"),
                changeset: None,
                skip: None,
                commits_limit: None,
                no_bookmark: false,
            }.import()
        })
        .boxify()
}

// Blobimport can't read remote bookmarks, so this function sets local bookmark
// with the same name
fn set_local_bookmark(src: PathBuf, bookmark: &str) -> BoxFuture<(), Error> {
    Command::new("hg")
        .arg("bookmark")
        .arg("--force")
        .arg(bookmark)
        .arg("-r")
        .arg(format!("remote/{}", bookmark))
        .current_dir(&src)
        .status_async()
        .into_future()
        .flatten()
        .from_err()
        .and_then(|status| check_status(status, "hg bookmark --force PROD"))
        .boxify()
}

// Verifies the consistency of the config repo
fn check_hg_config_repo(hg_repo_path: PathBuf) -> BoxFuture<(), Error> {
    // Config repo should have two bookmarks - PROD and RC.
    // PROD is for production jobs, RC for shadow jobs.
    // RC should be an descendant of a PROD i.e. configs of a shadow jobs are
    // configs of a production job plus some changes on top
    // PROD and RC can point to the same commit
    Command::new("hg")
        .arg("log")
        .arg("-r")
        .arg("PROD::RC")
        .arg("-T")
        .arg("{node}\n")
        .current_dir(&hg_repo_path)
        .output_async()
        .from_err()
        .and_then(|output| {
            let stdout = output.stdout.clone();
            check_status(output.status, "hg log -r 'PROD::RC' -T '{node}\\n'").and_then(move |()| {
                // Single line is hash + '\n'
                // Since PROD::RC range is inclusive, there should be at most 2 hash in the output
                let single_line_size = 40 + 1;
                if stdout.is_empty() {
                    Err(ErrorKind::RcNotDescendantOfProd().into())
                } else if stdout.len() > single_line_size * 2 {
                    Err(ErrorKind::RcTooFarAway().into())
                } else {
                    Ok(())
                }
            })
        })
        .map(|_| ())
        .boxify()
}
