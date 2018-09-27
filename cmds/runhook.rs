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
extern crate cachelib;
extern crate clap;
extern crate cmdlib;
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate hooks;
extern crate manifoldblob;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio;

#[cfg(test)]
extern crate async_unit;
extern crate bookmarks;
#[cfg(test)]
extern crate fixtures;
#[cfg(test)]
extern crate tempdir;

use blobrepo::{BlobRepo, ManifoldArgs};
use bookmarks::Bookmark;
use clap::{App, ArgMatches};
use failure::{Error, Result};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use hooks::{BlobRepoChangesetStore, BlobRepoFileContentStore, HookExecution, HookManager};
use hooks::lua_hook::LuaHook;
use mercurial_types::{HgChangesetId, RepositoryId};
use slog::{Drain, Level, Logger};
use slog::Discard;
use slog_glog_fmt::default_drain as glog_drain;
use std::env::args;
use std::fs::File;
use std::io::prelude::*;
use std::str::FromStr;
use std::sync::Arc;

fn run_hook(
    args: Vec<String>,
    repo_creator: fn(&Logger, &ArgMatches) -> BlobRepo,
) -> BoxFuture<HookExecution, Error> {
    // Define command line args and parse command line
    let matches = cmdlib::args::add_cachelib_args(
        App::new("runhook")
            .version("0.0.0")
            .about("run a hook")
            .args_from_usage(concat!(
                "<REPO_NAME>               'name of repository\n",
                "<HOOK_FILE>               'file containing hook code\n",
                "<HOOK_TYPE>               'the type of the hook (perfile, percs)\n",
                "<REV>                     'revision hash'\n",
            )),
        false, /* hide_advanced_args */
    ).get_matches_from(args);

    cmdlib::args::init_cachelib(&matches);

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
    let hook_type = matches.value_of("HOOK_TYPE").unwrap();
    println!("hook type is {}", hook_type);
    let file_hook = match hook_type.as_ref() {
        "perfile" => true,
        "percs" => false,
        _ => panic!("Invalid hook type"),
    };
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

    let changeset_store = Box::new(BlobRepoChangesetStore::new(repo.clone()));
    let content_store = Arc::new(BlobRepoFileContentStore::new(repo.clone()));
    let logger = Logger::root(Discard {}.ignore_res(), o!());
    let mut hook_manager = HookManager::new(
        repo_name,
        changeset_store,
        content_store,
        1024,
        1024 * 1024,
        logger,
    );
    let hook = LuaHook {
        name: String::from("testhook"),
        code,
    };
    if file_hook {
        hook_manager.register_file_hook("testhook", Arc::new(hook));
    } else {
        hook_manager.register_changeset_hook("testhook", Arc::new(hook));
    }
    let bookmark = Bookmark::new("testbm").unwrap();
    hook_manager.set_hooks_for_bookmark(bookmark.clone(), vec!["testhook".to_string()]);
    let id = try_boxfuture!(HgChangesetId::from_str(revstr));
    if file_hook {
        hook_manager
            .run_file_hooks_for_bookmark(id, &bookmark)
            .map(|executions| executions.get(0).unwrap().1.clone())
            .boxify()
    } else {
        hook_manager
            .run_changeset_hooks_for_bookmark(id, &bookmark)
            .map(|executions| executions.get(0).unwrap().1.clone())
            .boxify()
    }
}

fn create_blobrepo(logger: &Logger, matches: &ArgMatches) -> BlobRepo {
    let bucket = matches
        .value_of("manifold-bucket")
        .unwrap_or("mononoke_prod");
    let prefix = matches.value_of("manifold-prefix").unwrap_or("");
    let xdb_tier = matches
        .value_of("xdb-tier")
        .unwrap_or("xdb.mononoke_production");
    BlobRepo::new_manifold_no_postcommit(
        logger.clone(),
        &ManifoldArgs {
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            db_address: xdb_tier.to_string(),
        },
        RepositoryId::new(0),
    ).expect("failed to create blobrepo instance")
}

// It all starts here
fn main() -> Result<()> {
    let args_vec = args().collect();
    tokio::run(run_hook(args_vec, create_blobrepo).then(|res| {
        match res {
            Ok(HookExecution::Accepted) => println!("Hook accepted the changeset"),
            Ok(HookExecution::Rejected(rejection_info)) => {
                println!("Hook rejected the changeset {}", rejection_info.description)
            }
            Err(e) => println!("Failed to run hook {:?}", e),
        }
        Ok(())
    }));
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    use std::fs::File;
    use tempdir::TempDir;

    #[test]
    fn test_file_hook_accepted() {
        test_hook_accepted(true);
    }

    #[test]
    fn test_cs_hook_accepted() {
        test_hook_accepted(false);
    }

    fn test_hook_accepted(file: bool) {
        async_unit::tokio_unit_test(move || {
            let code = String::from(
                "hook = function (ctx)\n\
                 return true\n\
                 end",
            );
            let changeset_id = String::from("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            match test_hook(code, changeset_id, file) {
                Ok(HookExecution::Accepted) => (),
                Ok(HookExecution::Rejected(_)) => assert!(false, "Hook should be accepted"),
                Err(e) => assert!(false, format!("Unexpected error {:?}", e)),
            }
        });
    }

    #[test]
    fn test_file_hook_rejected() {
        test_hook_rejected(true)
    }

    #[test]
    fn test_cs_hook_rejected() {
        test_hook_rejected(false)
    }

    fn test_hook_rejected(file: bool) {
        async_unit::tokio_unit_test(move || {
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"sausages\"\n\
                 end",
            );
            let changeset_id = String::from("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            match test_hook(code, changeset_id, file) {
                Ok(HookExecution::Accepted) => assert!(false, "Hook should be rejected"),
                Ok(HookExecution::Rejected(rejection_info)) => {
                    assert!(rejection_info.description.starts_with("sausages"))
                }
                Err(e) => assert!(false, format!("Unexpected error {:?}", e)),
            }
        });
    }

    fn test_hook(code: String, changeset_id: String, run_file: bool) -> Result<HookExecution> {
        let dir = TempDir::new("runhook").unwrap();
        let file_path = dir.path().join("testhook.lua");
        let mut file = File::create(file_path.clone()).unwrap();
        file.write(code.as_bytes()).unwrap();
        let args = vec![
            String::from("runhook"),
            String::from("test_repo"),
            file_path.to_str().unwrap().into(),
            if run_file {
                String::from("perfile")
            } else {
                String::from("percs")
            },
            changeset_id,
        ];
        run_hook(args, test_blobrepo).wait()
    }

    fn test_blobrepo(_logger: &Logger, _matches: &ArgMatches) -> BlobRepo {
        fixtures::linear::getrepo(None)
    }

}
