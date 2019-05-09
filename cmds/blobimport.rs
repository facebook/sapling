// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate blobimport_lib;
extern crate clap;
extern crate cloned;
extern crate cmdlib;
extern crate failure_ext as failure;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate phases;
extern crate tokio;
extern crate tracing;

use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg};
use cloned::cloned;
use failure::{Result, SlogKVError};
use futures::Future;
use phases::SqlPhases;
use tracing::{trace_args, Traced};

use cmdlib::args;
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
            )
            .conflicts_with("changeset"),
        )
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let ctx = args::get_core_context(&matches);

    args::init_cachelib(&matches);

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

    let phases_store = Arc::new(args::open_sql::<SqlPhases>(&matches)?);

    let blobimport = args::create_repo(&ctx.logger(), &matches).and_then(move |repo| {
        let blobrepo = Arc::new(repo.clone());
        blobimport_lib::Blobimport {
            ctx: ctx.clone(),
            logger: ctx.logger().clone(),
            blobrepo,
            revlogrepo_path,
            changeset,
            skip,
            commits_limit,
            no_bookmark,
            phases_store,
        }
        .import()
        .traced(ctx.trace(), "blobimport", trace_args!())
        .map_err({
            cloned!(ctx);
            move |err| {
                error!(ctx.logger(), "error while blobimporting"; SlogKVError(err));
                ::std::process::exit(1);
            }
        })
        .then(move |result| args::upload_and_show_trace(ctx).then(move |_| result))
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(blobimport);
    // Let the runtime finish remaining work - uploading logs etc
    runtime.shutdown_on_idle();
    result
}
