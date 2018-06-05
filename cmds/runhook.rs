// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This implements a command line utility called 'runhook' which runs a Mononoke hook
//! against a specified changeset.
//! It's main purpose is to allow easy testing of hooks without having to run them as part of
//! a push in a Mononoke server
//! It currently supports hooks written in Lua only

#![deny(warnings)]

extern crate clap;
extern crate failure_ext as failure;
extern crate futures;
extern crate tokio_core;

extern crate blobrepo;
extern crate blobstore;
#[macro_use]
extern crate futures_ext;
extern crate manifoldblob;
extern crate mercurial_types;

extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;

extern crate hooks;

use hooks::{HookChangeset, HookExecution, HookManager};
use hooks::lua_hook::LuaHook;
use std::str;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::{BlobChangeset, BlobRepo};
use clap::{App, ArgMatches};
use failure::{Error, Result};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, DChangesetId, RepositoryId};
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;
use tokio_core::reactor::Core;

use futures::Future;
use std::fs::File;
use std::io::prelude::*;

const MAX_CONCURRENT_REQUESTS_PER_IO_THREAD: usize = 4;

fn run() -> Result<()> {
    // Define command line args and parse command line
    let matches = App::new("runhook")
        .version("0.0.0")
        .about("run a hook")
        .args_from_usage(concat!(
            "<HOOK_FILE>           'file containing hook code\n",
            "<REV>                 'revision hash'"
        ))
        .get_matches();

    let logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let hook_file = matches.value_of("HOOK_FILE").unwrap();
    let revstr = matches.value_of("REV").unwrap();

    let repo = create_blobrepo(&logger, &matches);
    let future = fetch_changeset(Arc::new(repo), revstr);
    let mut core = Core::new().unwrap();
    let changeset = core.run(future).unwrap();

    let user = str::from_utf8(changeset.user()).unwrap();
    let mut file = File::open(hook_file).expect("Unable to open the hook file");
    let mut code = String::new();
    file.read_to_string(&mut code)
        .expect("Unable to read the file");
    println!("hook_file is {} revision is {:?}", hook_file, revstr);
    println!("hook code is {}", code);
    println!("changeset user: {:?} ", user);

    let files = changeset.files();
    let vec_files = files
        .iter()
        .map(|arr| String::from_utf8_lossy(&arr.to_vec()).into_owned())
        .collect();
    let hook_cs = HookChangeset::new(user.to_string(), vec_files);
    let mut hook_manager = HookManager::new();
    let hook = LuaHook {
        name: String::from("testhook"),
        code,
    };
    hook_manager.install_hook("testhook", Arc::new(hook));
    let fut = hook_manager.run_hooks(Arc::new(hook_cs));
    match fut.wait() {
        Err(e) => {
            println!("Failed to execute hook {:?}", e);
            return Ok(());
        }
        Ok(executions) => {
            let hook_execution = executions.get("testhook").unwrap();
            match hook_execution {
                HookExecution::Accepted => println!("Hook acccepted the changeset"),
                HookExecution::Rejected(rejection_info) => {
                    println!("Hook rejected the changeset {}", rejection_info.description)
                }
            }
        }
    }
    Ok(())
}

fn create_blobrepo(logger: &Logger, matches: &ArgMatches) -> BlobRepo {
    let bucket = matches
        .value_of("manifold-bucket")
        .unwrap_or("mononoke_prod");
    let prefix = matches.value_of("manifold-prefix").unwrap_or("");
    let xdb_tier = matches
        .value_of("xdb-tier")
        .unwrap_or("xdb.mononoke_test_2");
    let io_threads = 5;
    let default_cache_size = 1000000;
    BlobRepo::new_test_manifold(
        logger.clone(),
        bucket,
        prefix,
        RepositoryId::new(0),
        xdb_tier,
        default_cache_size,
        default_cache_size,
        default_cache_size,
        None,
        io_threads,
        MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
    ).expect("failed to create blobrepo instance")
}

fn fetch_changeset(repo: Arc<BlobRepo>, rev: &str) -> BoxFuture<BlobChangeset, Error> {
    let cs_id = try_boxfuture!(DChangesetId::from_str(rev));
    repo.get_changeset_by_changesetid(&cs_id)
}

// It all starts here
fn main() -> Result<()> {
    run()
}
