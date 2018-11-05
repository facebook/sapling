// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(duration_as_u128)]

extern crate clap;
extern crate failure_ext as failure;
extern crate filenodes;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_term;
extern crate sql;
extern crate sqlfilenodes;
extern crate tokio;

use failure::prelude::*;
use filenodes::Filenodes;
use futures::future::{loop_fn, Future, Loop};
use mercurial_types::{HgFileNodeId, HgNodeHash, RepoPath, RepositoryId};
use slog::{Drain, Level};
use slog_glog_fmt::default_drain as glog_drain;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::str::FromStr;
use std::time::Instant;

fn main() -> Result<()> {
    let matches = clap::App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            [filename]                  'filename'
            [filenode]                  'filenode'
            [xdb-tier]                  'xdb tier'
            [shard-count]               'shard count'
            -p, --myrouter-port [PORT]  'port for local myrouter instance'
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
    let myrouter_port = matches
        .value_of("myrouter-port")
        .expect("myrouter-port is not specified")
        .parse::<u16>()
        .expect("myrouter-port is not a u16 port number");
    let is_directory = matches.is_present("directory");
    let depth: Option<usize> = matches
        .value_of("depth")
        .map(|depth| depth.parse().expect("depth must be a positive integer"));
    let shard_count: Option<usize> = matches.value_of("shard-count").map(|shard_count| {
        shard_count
            .parse()
            .expect("shard count must be a positive integer")
    });

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
    let filenodes = match shard_count {
        None => SqlFilenodes::with_myrouter(xdb_tier.clone(), myrouter_port),
        Some(shard_count) => {
            SqlFilenodes::with_sharded_myrouter(xdb_tier.clone(), myrouter_port, shard_count)
        }
    };

    let mut runtime = tokio::runtime::Runtime::new().expect("failed to create Runtime");

    info!(root_log, "Waiting for MyRouter to be accessible...");
    runtime.block_on(sql::myrouter::wait_for_myrouter(myrouter_port, xdb_tier))?;
    info!(root_log, "MyRouter is accessible");

    info!(root_log, "Fetching parents...");
    let before = Instant::now();

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
    });

    let res = runtime.block_on(fut)?;

    info!(
        root_log,
        "Finished: {}, took: {:?}",
        res,
        Instant::now().duration_since(before).as_millis()
    );

    Ok(())
}
