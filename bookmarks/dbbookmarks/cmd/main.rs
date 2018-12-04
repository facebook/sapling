// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bookmarks;
extern crate clap;
extern crate context;
extern crate dbbookmarks;
extern crate failure_ext as failure;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_term;
extern crate sql;
extern crate tokio;

use std::path::PathBuf;

use bookmarks::{Bookmark, Bookmarks};
use context::CoreContext;
use dbbookmarks::{SqlBookmarks, SqlConstructors};
use failure::prelude::*;
use mercurial_types::RepositoryId;
use slog::{Drain, Level};
use slog_glog_fmt::default_drain as glog_drain;

fn main() -> Result<()> {
    let matches = clap::App::new("read bookmark from sqlite")
        .version("0.0.0")
        .about("read bookmark")
        .args_from_usage(
            r#"
            [filename]                  'filename'
            [bookmark]                  'bookmark'
            [repo_id]                   'repo_id'
        "#,
        )
        .get_matches();

    let filename = PathBuf::from(
        matches
            .value_of("filename")
            .expect("filename is not specified"),
    );
    let bookmark = Bookmark::new(
        matches
            .value_of("bookmark")
            .expect("bookmark is not specified"),
    )?;
    let repo_id = RepositoryId::new(matches
        .value_of("repo_id")
        .expect("repo_id is not specified")
        .parse()?);

    let root_log = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let ctx = CoreContext::test_mock();
    let bookmarks = SqlBookmarks::with_sqlite_path(filename)?;

    let fut = bookmarks.get(ctx, &bookmark, &repo_id);

    let mut runtime = tokio::runtime::Runtime::new().expect("failed to create Runtime");

    let res = runtime.block_on(fut)?;

    info!(root_log, "Finished: {:#?}", res,);

    Ok(())
}
