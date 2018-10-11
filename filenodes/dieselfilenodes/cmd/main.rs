// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate clap;
extern crate db;
extern crate dieselfilenodes;
extern crate filenodes;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_term;
extern crate time_ext;
extern crate tokio;

use dieselfilenodes::{MysqlFilenodes, DEFAULT_INSERT_CHUNK_SIZE};
use filenodes::Filenodes;
use futures::{future::{loop_fn, Future, Loop}, sync::oneshot};
use mercurial_types::{HgFileNodeId, HgNodeHash, RepoPath, RepositoryId};
use slog::{Drain, Level};
use slog_glog_fmt::default_drain as glog_drain;
use std::str::FromStr;
use std::time::Instant;
use time_ext::DurationExt;

fn main() {
    let matches = clap::App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            [filename]                  'filename'
            [filenode]                  'filenode'
            [xdb-tier]                  'xdb tier'
            --depth [DEPTH]             'how many ancestors to fetch, fetch all if not set'
            --directory                 'not a file but directory'
            --debug
        "#,
        )
        .get_matches();

    let filename = matches
        .value_of("filename")
        .expect("filename is not specified");
    let filenode = matches
        .value_of("filenode")
        .expect("filenode is not specified");
    let xdb_tier = matches
        .value_of("xdb-tier")
        .expect("xdb-tier is not specified");
    let is_directory = matches.is_present("directory");
    let depth: Option<usize> = matches
        .value_of("depth")
        .map(|depth| depth.parse().expect("depth must be a positive integer"));

    let root_log = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let filename = if is_directory {
        info!(root_log, "directory");
        RepoPath::dir(filename).expect("incorrect repopath")
    } else {
        info!(root_log, "file");
        RepoPath::file(filename).expect("incorrect repopath")
    };

    let filenode_hash = HgNodeHash::from_str(filenode).expect("incorrect filenode: should be sha1");
    let filenode_hash = HgFileNodeId::new(filenode_hash);

    info!(root_log, "Connecting to mysql...");
    let filenodes =
        MysqlFilenodes::open(xdb_tier, DEFAULT_INSERT_CHUNK_SIZE).expect("cannot connect to mysql");
    info!(root_log, "Connected");

    info!(root_log, "Fetching parents...");
    let before = Instant::now();
    let (send, recv) = oneshot::channel();

    let fut = loop_fn((1, filenode_hash), move |(res, filenode_hash)| {
        filenodes
            .get_filenode(&filename, &filenode_hash, &RepositoryId::new(0))
            .map(move |filenode| {
                if Some(res) == depth {
                    return Loop::Break(res);
                }
                match filenode.expect("not found").p1 {
                    Some(p1) => Loop::Continue((res + 1, p1)),
                    None => Loop::Break(res),
                }
            })
    }).map_err(|_| ())
        .and_then(|res| send.send(res).map_err(|_| ()));
    tokio::run(fut);

    let res = recv.wait().expect("failed to run future to completion");

    info!(
        root_log,
        "Finished: {}, took: {:?}",
        res,
        Instant::now().duration_since(before).as_millis()
    );
}
