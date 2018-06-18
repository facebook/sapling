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
#![feature(try_from)]

extern crate blobrepo;
extern crate blobstore;
extern crate clap;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate hooks;
extern crate manifoldblob;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio_core;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate linear;
#[cfg(test)]
extern crate tempdir;

use blobrepo::BlobRepo;
use clap::{App, ArgMatches};
use failure::{Error, Result};
use futures::{failed, Future};
use futures_ext::{BoxFuture, FutureExt};
use hooks::{HookExecution, HookManager};
use hooks::lua_hook::LuaHook;
use mercurial_types::{HgChangesetId, RepositoryId};
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;
use std::env::args;
use std::fs::File;
use std::io::prelude::*;
use std::str::FromStr;
use std::sync::Arc;
use tokio_core::reactor::Core;

const MAX_CONCURRENT_REQUESTS_PER_IO_THREAD: usize = 4;

fn run_hook(
    args: Vec<String>,
    repo_creator: fn(&Logger, &ArgMatches) -> BlobRepo,
) -> BoxFuture<HookExecution, Error> {
    // Define command line args and parse command line
    let matches = App::new("runhook")
        .version("0.0.0")
        .about("run a hook")
        .args_from_usage(concat!(
            "<REPO_NAME>           'name of repository\n",
            "<HOOK_FILE>           'file containing hook code\n",
            "<REV>                 'revision hash'"
        ))
        .get_matches_from(args);

    let logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let repo_name = String::from(matches.value_of("REPO_NAME").unwrap());
    let hook_file = matches.value_of("HOOK_FILE").unwrap();
    let revstr = matches.value_of("REV").unwrap();
    let repo = repo_creator(&logger, &matches);

    let mut file = File::open(hook_file).expect("Unable to open the hook file");
    let mut code = String::new();
    file.read_to_string(&mut code)
        .expect("Unable to read the file");
    println!("======= Running hook =========");
    println!("Repository name is {}", repo_name);
    println!("Hook file is {} revision is {:?}", hook_file, revstr);
    println!("Hook code is {}", code);
    println!("==============================");

    let mut hook_manager = HookManager::new(repo_name, repo.clone(), 1024, 1024 * 1024);
    let hook = LuaHook {
        name: String::from("testhook"),
        code,
    };
    hook_manager.install_hook("testhook", Arc::new(hook));

    match HgChangesetId::from_str(revstr) {
        Ok(id) => hook_manager
            .run_hooks(id)
            .map(|executions| executions.get("testhook").unwrap().clone())
            .boxify(),
        Err(e) => Box::new(failed(e)),
    }
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
        io_threads,
        MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
    ).expect("failed to create blobrepo instance")
}

// It all starts here
fn main() -> Result<()> {
    let args_vec = args().collect();
    let fut = run_hook(args_vec, create_blobrepo);
    let mut core = Core::new().unwrap();
    match core.run(fut) {
        Ok(HookExecution::Accepted) => println!("Hook accepted the changeset"),
        Ok(HookExecution::Rejected(rejection_info)) => {
            println!("Hook rejected the changeset {}", rejection_info.description)
        }
        Err(e) => println!("Failed to run hook {:?}", e),
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    use linear;
    use std::fs::File;
    use tempdir::TempDir;

    #[test]
    fn test_hook_accepted() {
        async_unit::tokio_unit_test(|| {
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"Jeremy Fitzhardinge <jsgf@fb.com>\"\n\
                 end",
            );
            let changeset_id = String::from("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let f = |execution| match execution {
                Ok(HookExecution::Accepted) => (),
                Ok(HookExecution::Rejected(_)) => assert!(false, "Hook should be accepted"),
                Err(e) => assert!(false, format!("Unexpected error {:?}", e)),
            };
            test_hook(code, changeset_id, &f);
        });
    }

    #[test]
    fn test_hook_rejected() {
        async_unit::tokio_unit_test(|| {
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"Mahatma Ghandi\"\n\
                 end",
            );
            let changeset_id = String::from("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let f = |execution| match execution {
                Ok(HookExecution::Accepted) => assert!(false, "Hook should be rejected"),
                Ok(HookExecution::Rejected(rejection_info)) => {
                    assert!(rejection_info.description.starts_with("short desc"))
                }
                Err(e) => assert!(false, format!("Unexpected error {:?}", e)),
            };
            test_hook(code, changeset_id, &f);
        });
    }

    fn test_hook(code: String, changeset_id: String, f: &Fn(Result<HookExecution>) -> ()) {
        let dir = TempDir::new("runhook").unwrap();
        let file_path = dir.path().join("testhook.lua");
        let mut file = File::create(file_path.clone()).unwrap();
        file.write(code.as_bytes()).unwrap();
        let args = vec![
            String::from("test_repo"),
            String::from("runhook"),
            file_path.to_str().unwrap().into(),
            changeset_id,
        ];
        let fut = run_hook(args, test_blobrepo);
        let result = fut.wait();
        f(result);
    }

    fn test_blobrepo(_logger: &Logger, _matches: &ArgMatches) -> BlobRepo {
        linear::getrepo(None)
    }

}
