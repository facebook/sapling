// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobimport_lib;
use clap::{App, Arg};
use cloned::cloned;
use cmdlib::args;
use failure_ext::{Result, SlogKVError};
use futures::Future;
use futures_ext::FutureExt;
use mercurial_types::HgNodeHash;
use phases::SqlPhases;
use slog::error;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{trace_args, Traced};

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
        default_glog: true,
    };
    app.build("revlog to blob importer")
        .version("0.0.0")
        .about("Import a revlog-backed Mercurial repo into Mononoke blobstore.")
        .args_from_usage(
            r#"
            <INPUT>                          'input revlog repo'
            --changeset [HASH]               'if provided, the only changeset to be imported'
            --no-bookmark                    'if provided won't update bookmarks'
            --no-create                      'if provided won't create a new repo (only meaningful for local)'
            --lfs-helper [LFS_HELPER]        'if provided, path to an executable that accepts OID SIZE and returns a LFS blob to stdout'
            --concurrent-changesets [LIMIT]  'if provided, max number of changesets to upload concurrently'
            --concurrent-blobs [LIMIT]       'if provided, max number of blobs to process concurrently'
            --concurrent-lfs-imports [LIMIT] 'if provided, max number of LFS files to import concurrently'
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

    let lfs_helper = matches.value_of("lfs-helper").map(|l| l.to_string());

    let concurrent_changesets = args::get_usize(&matches, "concurrent-changesets", 100);
    let concurrent_blobs = args::get_usize(&matches, "concurrent-blobs", 100);
    let concurrent_lfs_imports = args::get_usize(&matches, "concurrent-lfs-imports", 10);

    let phases_store = args::open_sql::<SqlPhases>(&matches);

    let blobrepo = if matches.is_present("no-create") {
        args::open_repo_unredacted(&ctx.logger(), &matches).left_future()
    } else {
        args::create_repo_unredacted(&ctx.logger(), &matches).right_future()
    };

    let blobimport = blobrepo
        .join(phases_store)
        .and_then(move |(blobrepo, phases_store)| {
            let phases_store = Arc::new(phases_store);

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
                lfs_helper,
                concurrent_changesets,
                concurrent_blobs,
                concurrent_lfs_imports,
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
