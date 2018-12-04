// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate clap;
extern crate cmdlib;
extern crate context;
extern crate failure_ext as failure;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate tokio;

use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg};
use context::CoreContext;
use failure::{Result, SlogKVError};
use futures::Future;

use cmdlib::{args, blobimport_lib::Blobimport};
use mercurial_types::HgNodeHash;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
        local_instances: true,
        default_glog: true,
    };
    app.build("revlog to blob importer")
        .version("0.0.0")
        .about("Import a revlog-backed Mercurial repo into Mononoke blobstore.")
        .args_from_usage(
            r#"
            <INPUT>                         'input revlog repo'
            --changeset [HASH]              'if provided, the only changeset to be imported'
            --no-bookmark                   'if provided won't update bookmarks'
        "#,
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

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let ctx = CoreContext::test_mock();
    let logger = args::get_logger(&matches);

    args::init_cachelib(&matches);
    let repo = args::create_repo(ctx, &logger, &matches)?;
    let blobrepo = Arc::new(repo.blobrepo().clone());

    let revlogrepo_path = matches
        .value_of("INPUT")
        .expect("input is not specified")
        .into();

    let changeset = match matches.value_of("changeset") {
        None => None,
        Some(hash) => Some(HgNodeHash::from_str(hash)?),
    };

    let skip = if !matches.is_present("skip") {
        None
    } else {
        Some(args::get_usize(&matches, "skip", 0))
    };

    let commits_limit = if !matches.is_present("commits-limit") {
        None
    } else {
        Some(args::get_usize(&matches, "commits-limit", 0))
    };

    let no_bookmark = matches.is_present("no-bookmark");

    let blobimport = Blobimport {
        // TODO(T37478150, luk) this is not a test use case, but I want to fix it properly later
        ctx: CoreContext::test_mock(),
        logger: logger.clone(),
        blobrepo,
        revlogrepo_path,
        changeset,
        skip,
        commits_limit,
        no_bookmark,
    }.import()
        .map_err(move |err| {
            error!(logger, "error while blobimporting"; SlogKVError(err));
            ::std::process::exit(1);
        });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(blobimport);
    // Let the runtime finish remaining work - uploading logs etc
    runtime.shutdown_on_idle();
    result
}
