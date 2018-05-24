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
extern crate changesets;
extern crate clap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
#[macro_use]
extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio_core;

mod bookmark;
mod changeset;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use clap::{App, Arg, ArgMatches};
use failure::err_msg;
use futures::{future, Future, Stream};
use futures_ext::FutureExt;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;
use tokio_core::reactor::Core;

use blobrepo::BlobRepo;
use changesets::SqliteChangesets;
use mercurial::RevlogRepo;
use mercurial_types::RepositoryId;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            <INPUT>                         'input revlog repo'
            --debug                         'print debug logs'
            --repo_id <repo_id>             'ID of the newly imported repo'
            --manifold-bucket [BUCKET]      'manifold bucket'
            --manifold-prefix [PREFIX]      'manifold prefix Default: new_blobimport_test'
            --db-address [address]          'address of a db. Used only for manifold blobstore'
            --blobstore-cache-size [SIZE]   'size of the blobstore cache'
            --changesets-cache-size [SIZE]  'size of the changesets cache'
            --filenodes-cache-size [SIZE]   'size of the filenodes cache'
            --io-thread-num [NUM]           'num of the io threads to use'
            --max-concurrent-request-per-io-thread [NUM] 'max requests per io thread'
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
                .possible_values(&["rocksdb", "manifold"])
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

fn get_usize<'a>(matches: &ArgMatches<'a>, key: &str, default: usize) -> usize {
    matches
        .value_of(key)
        .map(|val| {
            val.parse::<usize>()
                .expect(&format!("{} must be integer", key))
        })
        .unwrap_or(default)
}

fn open_blobrepo<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> BlobRepo {
    let repo_id = RepositoryId::new(matches.value_of("repo_id").unwrap().parse().unwrap());

    match matches.value_of("blobstore").unwrap() {
        "rocksdb" => {
            let output = matches.value_of("OUTPUT").expect("output is not specified");
            let output = Path::new(output)
                .canonicalize()
                .expect("Failed to read output path");

            assert!(
                output.is_dir(),
                "The path {:?} does not exist or is not a directory",
                output
            );

            for subdir in &[".hg", "blobs", "heads"] {
                let subdir = output.join(subdir);
                if subdir.exists() {
                    assert!(
                        subdir.is_dir(),
                        "Failed to start Rocksdb BlobRepo: \
                         {:?} already exists and is not a directory",
                        subdir
                    );
                    let content: Vec<_> = subdir
                        .read_dir()
                        .expect("Failed to read directory content")
                        .collect();
                    assert!(
                        content.is_empty(),
                        "Failed to start Rocksdb BlobRepo: \
                         {:?} already exists and is not empty: {:?}",
                        subdir,
                        content
                    );
                } else {
                    fs::create_dir(&subdir)
                        .expect(&format!("Failed to create directory {:?}", subdir));
                }
            }

            {
                let changesets_path = output.join("changesets");
                assert!(
                    !changesets_path.exists(),
                    "Failed to start Rocksdb BlobRepo: {:?} already exists"
                );
                SqliteChangesets::create(changesets_path.to_string_lossy())
                    .expect("Failed to initialize changests sqlite database");
            }

            BlobRepo::new_rocksdb(
                logger.new(o!["BlobRepo:Rocksdb" => output.to_string_lossy().into_owned()]),
                &output,
                repo_id,
            ).expect("failed to create rocksdb blobrepo")
        }
        "manifold" => {
            let manifold_bucket = matches
                .value_of("manifold-bucket")
                .expect("manifold bucket is not specified");

            BlobRepo::new_test_manifold(
                logger.new(o!["BlobRepo:TestManifold" => manifold_bucket.to_owned()]),
                manifold_bucket,
                matches
                    .value_of("manifold-prefix")
                    .unwrap_or("new_blobimport_test"),
                repo_id,
                matches
                    .value_of("db-address")
                    .expect("--db-address is not specified"),
                get_usize(&matches, "blobstore-cache-size", 100_000_000),
                get_usize(&matches, "changesets-cache-size", 100_000_000),
                get_usize(&matches, "filenodes-cache-size", 100_000_000),
                get_usize(&matches, "io-thread-num", 5),
                get_usize(&matches, "max-concurrent-request-per-io-thread", 5),
            ).expect("failed to create manifold blobrepo")
        }
        bad => panic!("unexpected blobstore type: {}", bad),
    }
}

fn main() {
    let matches = setup_app().get_matches();

    let revlogrepo = {
        let input = matches.value_of("INPUT").expect("input is not specified");
        RevlogRepo::open(input).expect("cannot open revlogrepo")
    };

    let mut core = Core::new().expect("cannot create tokio core");

    let logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let blobrepo = Arc::new(open_blobrepo(&logger, &matches));

    let upload_changesets = changeset::upload_changesets(
        &matches,
        revlogrepo.clone(),
        blobrepo.clone(),
        get_usize(&matches, "parsing-cpupool-size", 8),
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
        bookmark::upload_bookmarks(&logger, revlogrepo, blobrepo)
    };

    core.run(upload_changesets.and_then(|()| {
        info!(logger, "finished uploading changesets, now doing bookmarks");
        upload_bookmarks
    })).expect("main stream failed");
}
