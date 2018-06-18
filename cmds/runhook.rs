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

#[cfg(test)]
extern crate tempdir;

extern crate hooks;

use hooks::{HookChangeset, HookExecution, HookManager};
use hooks::lua_hook::LuaHook;
use std::convert::TryFrom;
use std::str;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::{BlobChangeset, BlobRepo};
use clap::{App, ArgMatches};
use failure::{Error, Result};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, RepositoryId};
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;
use tokio_core::reactor::Core;

use std::env::args;
use std::fs::File;
use std::io::prelude::*;

const MAX_CONCURRENT_REQUESTS_PER_IO_THREAD: usize = 4;

fn run_hook(args: Vec<String>) -> Result<HookExecution> {
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

    let repo_name = matches.value_of("REPO_NAME").unwrap();
    let hook_file = matches.value_of("HOOK_FILE").unwrap();
    let revstr = matches.value_of("REV").unwrap();

    let repo = create_blobrepo(&logger, &matches);
    let future = fetch_changeset(Arc::new(repo), revstr);
    let mut core = Core::new().unwrap();
    let changeset = core.run(future).unwrap();
    let hook_cs = HookChangeset::try_from(changeset)?;

    let mut file = File::open(hook_file).expect("Unable to open the hook file");
    let mut code = String::new();
    file.read_to_string(&mut code)
        .expect("Unable to read the file");
    println!("======= Running hook =========");
    println!("Repository name is {}", repo_name);
    println!("Hook file is {} revision is {:?}", hook_file, revstr);
    println!("Hook code is {}", code);
    println!("Changeset author: {:?} ", hook_cs.author);
    println!("==============================");

    let mut hook_manager = HookManager::new();
    let hook = LuaHook {
        name: String::from("testhook"),
        code,
    };
    hook_manager.install_hook("testhook", Arc::new(hook));
    let fut = hook_manager
        .run_hooks(repo_name.to_string(), Arc::new(hook_cs))
        .map(|executions| executions.get("testhook").unwrap().clone());
    core.run(fut)
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

fn fetch_changeset(repo: Arc<BlobRepo>, rev: &str) -> BoxFuture<BlobChangeset, Error> {
    let cs_id = try_boxfuture!(HgChangesetId::from_str(rev));
    repo.get_changeset_by_changesetid(&cs_id)
}

// It all starts here
fn main() -> Result<()> {
    let args_vec = args().collect();
    match run_hook(args_vec) {
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

    use std::fs::File;
    use tempdir::TempDir;

    #[test]
    fn test_hook_accepted() {
        let code = String::from(
            "hook = function (info, files)\n\
             return info.author == \"Tim Fox <tfox@fb.com>\"\n\
             end",
        );
        let changeset_id = String::from("50f849250f42436ee0db142aab721a12b7d95672");
        let f = |execution| match execution {
            Ok(HookExecution::Accepted) => (),
            Ok(HookExecution::Rejected(rejection_info)) => {
                assert!(rejection_info.description.starts_with("iuwehuweh"))
            }
            _ => assert!(false),
        };
        test_hook(code, changeset_id, &f);
    }

    #[test]
    fn test_hook_rejected() {
        let code = String::from(
            "hook = function (info, files)\n\
             return info.author == \"Mahatma Ghandi\"\n\
             end",
        );
        let changeset_id = String::from("50f849250f42436ee0db142aab721a12b7d95672");
        let f = |execution| match execution {
            Ok(HookExecution::Accepted) => assert!(false),
            Ok(HookExecution::Rejected(rejection_info)) => {
                assert!(rejection_info.description.starts_with("short desc"))
            }
            _ => assert!(false),
        };
        test_hook(code, changeset_id, &f);
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
        f(run_hook(args));
    }

}
