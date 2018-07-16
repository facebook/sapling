// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
extern crate clap;
extern crate cmdlib;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_types;
extern crate scuba_ext;
#[macro_use]
extern crate slog;
extern crate tokio;

mod bookmark;
mod changeset;

use std::sync::Arc;

use clap::{App, Arg};
use failure::{err_msg, Result, SlogKVError};
use futures::{future, Future, Stream};
use futures_ext::FutureExt;

use cmdlib::args;
use mercurial::RevlogRepo;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
        local_instances: true,
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

    let revlogrepo_path = matches.value_of("INPUT").expect("input is not specified");

    let logger = args::get_logger(&matches);

    let blobrepo = Arc::new(args::create_blobrepo(&logger, &matches));

    let stale_bookmarks = {
        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");
        bookmark::read_bookmarks(revlogrepo)
    };

    let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

    let upload_changesets =
        changeset::upload_changesets(&matches, revlogrepo.clone(), blobrepo.clone())
            .buffer_unordered(100)
            .map({
                let mut cs_count = 0;
                let logger = logger.clone();
                move |cs| {
                    debug!(logger, "inserted: {}", cs.get_changeset_id());
                    cs_count = cs_count + 1;
                    if cs_count % 5000 == 0 {
                        info!(logger, "inserted commits # {}", cs_count);
                    }
                    ()
                }
            })
            .map_err({
                let logger = logger.clone();
                move |err| {
                    error!(logger, "failed to blobimport: {}", err);

                    for cause in err.causes() {
                        info!(logger, "cause: {}", cause);
                    }
                    info!(logger, "root cause: {:?}", err.root_cause());

                    let msg = format!("failed to blobimport: {}", err);
                    err_msg(msg)
                }
            })
            .for_each(|()| Ok(()))
            .inspect({
                let logger = logger.clone();
                move |()| {
                    info!(logger, "finished uploading changesets");
                }
            });

    let upload_all = stale_bookmarks
        .and_then(move |stale_bookmarks| upload_changesets.map(|()| stale_bookmarks))
        .and_then({
            let no_bookmark = matches.is_present("no-bookmark");
            let logger = logger.clone();

            move |stale_bookmarks| {
                if no_bookmark {
                    info!(
                        logger,
                        "since --no-bookmark was provided, bookmarks won't be imported"
                    );
                    future::ok(()).boxify()
                } else {
                    bookmark::upload_bookmarks(&logger, revlogrepo, blobrepo, stale_bookmarks)
                }
            }
        })
        .map_err({
            let logger = logger.clone();
            move |err| {
                error!(logger, "error while blobimporting"; SlogKVError(err));
                panic!("import failed");
            }
        });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(upload_all);
    // Let the runtime finish remaining work - uploading logs etc
    runtime.shutdown_on_idle();
    result
}
