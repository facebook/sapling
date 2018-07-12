// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
extern crate clap;
extern crate cmdlib;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
#[macro_use]
extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_types;
extern crate scuba_ext;
#[macro_use]
extern crate slog;
extern crate tokio_core;

mod bookmark;
mod changeset;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use clap::{App, Arg, ArgMatches};
use failure::{err_msg, Result, ResultExt};
use futures::{future, Future, Stream};
use futures_ext::FutureExt;
use slog::Logger;
use tokio_core::reactor::Core;

use blobrepo::BlobRepo;
use cmdlib::args;
use mercurial::RevlogRepo;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
    };
    app.build("revlog to blob importer")
        .version("0.0.0")
        .about("Import a revlog-backed Mercurial repo into Mononoke blobstore.")
        .args_from_usage(
            r#"
            <INPUT>                         'input revlog repo'
            --parsing-cpupool-size [NUM]    'size of cpupool for parsing revlogs'
            --changeset [HASH]              'if provided, the only changeset to be imported'
            --no-bookmark                   'if provided won't update bookmarks'
            [OUTPUT]                        'Blobstore output'
        "#,
        )
        .arg(
            Arg::with_name("blobstore")
                .long("blobstore")
                .takes_value(true)
                .possible_values(&["files", "rocksdb", "manifold"])
                .required(true)
                .help("blobstore type"),
        )
        .arg(
            Arg::from_usage("--skip [SKIP]  'skips commits from the beginning'")
                .conflicts_with("changeset"),
        )
        .arg(
            Arg::from_usage(
                "--commits-limit [LIMIT] 'import only LIMIT first commits from revlog repo'",
            ).conflicts_with("changeset"),
        )
}

fn setup_local_state(output: &Path) -> Result<()> {
    if !output.is_dir() {
        bail_msg!("{:?} does not exist or is not a directory", output);
    }

    for subdir in &["blobs"] {
        let subdir = output.join(subdir);
        if subdir.exists() {
            if !subdir.is_dir() {
                bail_msg!("{:?} already exists and is not a directory", subdir);
            }

            let content: Vec<_> = subdir.read_dir()?.collect();
            if !content.is_empty() {
                bail_msg!(
                    "{:?} already exists and is not empty: {:?}",
                    subdir,
                    content
                );
            }
        } else {
            fs::create_dir(&subdir)
                .with_context(|_| format!("failed to create subdirectory {:?}", subdir))?;
        }
    }
    Ok(())
}

fn open_blobrepo<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> BlobRepo {
    let repo_id = args::get_repo_id(matches);

    match matches.value_of("blobstore").unwrap() {
        "files" => {
            let output = matches.value_of("OUTPUT").expect("output is not specified");
            let output = Path::new(output)
                .canonicalize()
                .expect("Failed to read output path");
            setup_local_state(&output).expect("Setting up file blobrepo failed");

            BlobRepo::new_files(
                logger.new(o!["BlobRepo:Files" => output.to_string_lossy().into_owned()]),
                &output,
                repo_id,
            ).expect("failed to create file blobrepo")
        }
        "rocksdb" => {
            let output = matches.value_of("OUTPUT").expect("output is not specified");
            let output = Path::new(output)
                .canonicalize()
                .expect("Failed to read output path");
            setup_local_state(&output).expect("Setting up rocksdb blobrepo failed");

            BlobRepo::new_rocksdb(
                logger.new(o!["BlobRepo:Rocksdb" => output.to_string_lossy().into_owned()]),
                &output,
                repo_id,
            ).expect("failed to create rocksdb blobrepo")
        }
        "manifold" => {
            let manifold_args = args::parse_manifold_args(&matches, 100_000_000);

            BlobRepo::new_manifold(
                logger.new(o!["BlobRepo:TestManifold" => manifold_args.bucket.clone()]),
                &manifold_args,
                repo_id,
            ).expect("failed to create manifold blobrepo")
        }
        bad => panic!("unexpected blobstore type: {}", bad),
    }
}

fn main() {
    let matches = setup_app().get_matches();

    let revlogrepo_path = matches.value_of("INPUT").expect("input is not specified");

    let mut core = Core::new().expect("cannot create tokio core");

    let logger = args::get_logger(&matches);

    let blobrepo = Arc::new(open_blobrepo(&logger, &matches));

    let stale_bookmarks = {
        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");
        core.run(bookmark::read_bookmarks(revlogrepo))
            .expect("failed to read bookmarks")
    };

    let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

    let upload_changesets = changeset::upload_changesets(
        &matches,
        revlogrepo.clone(),
        blobrepo.clone(),
        args::get_usize(&matches, "parsing-cpupool-size", 8),
    ).buffer_unordered(100)
        .map({
            let mut cs_count = 0;
            let logger = logger.clone();
            move |cs| {
                debug!(logger, "inserted: {}", cs.get_changeset_id());
                cs_count = cs_count + 1;
                if cs_count % 5000 == 0 {
                    info!(logger, "inserted commits # {}", cs_count);
                }
                ()
            }
        })
        .map_err(|err| {
            error!(logger, "failed to blobimport: {}", err);

            for cause in err.causes() {
                info!(logger, "cause: {}", cause);
            }
            info!(logger, "root cause: {:?}", err.root_cause());

            let msg = format!("failed to blobimport: {}", err);
            err_msg(msg)
        })
        .for_each(|()| Ok(()));

    let upload_bookmarks = if matches.is_present("no-bookmark") {
        let logger = logger.clone();
        future::lazy(move || {
            info!(
                logger,
                "since --no-bookmark was provided, bookmarks won't be imported"
            );
            Ok(())
        }).boxify()
    } else {
        bookmark::upload_bookmarks(&logger, revlogrepo, blobrepo, stale_bookmarks)
    };

    core.run(upload_changesets.and_then(|()| {
        info!(logger, "finished uploading changesets, now doing bookmarks");
        upload_bookmarks
    })).expect("main stream failed");
}
