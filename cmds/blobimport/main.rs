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
extern crate futures_cpupool;
#[macro_use]
extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_types;
extern crate scuba_ext;
#[macro_use]
extern crate slog;
extern crate tokio_core;

mod bookmark;
mod changeset;

use std::sync::Arc;

use clap::{App, Arg};
use failure::err_msg;
use futures::{future, Future, Stream};
use futures_ext::FutureExt;
use tokio_core::reactor::Core;

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
            --parsing-cpupool-size [NUM]    'size of cpupool for parsing revlogs'
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

fn main() {
    let matches = setup_app().get_matches();

    let revlogrepo_path = matches.value_of("INPUT").expect("input is not specified");

    let mut core = Core::new().expect("cannot create tokio core");

    let logger = args::get_logger(&matches);

    let blobrepo = Arc::new(args::create_blobrepo(&logger, &matches));

    let stale_bookmarks = {
        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");
        core.run(bookmark::read_bookmarks(revlogrepo))
            .expect("failed to read bookmarks")
    };

    let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

    let upload_changesets = changeset::upload_changesets(
        &matches,
        revlogrepo.clone(),
        blobrepo.clone(),
        args::get_usize(&matches, "parsing-cpupool-size", 8),
    ).buffer_unordered(100)
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
        .map_err(|err| {
            error!(logger, "failed to blobimport: {}", err);

            for cause in err.causes() {
                info!(logger, "cause: {}", cause);
            }
            info!(logger, "root cause: {:?}", err.root_cause());

            let msg = format!("failed to blobimport: {}", err);
            err_msg(msg)
        })
        .for_each(|()| Ok(()));

    let upload_bookmarks = if matches.is_present("no-bookmark") {
        let logger = logger.clone();
        future::lazy(move || {
            info!(
                logger,
                "since --no-bookmark was provided, bookmarks won't be imported"
            );
            Ok(())
        }).boxify()
    } else {
        bookmark::upload_bookmarks(&logger, revlogrepo, blobrepo, stale_bookmarks)
    };

    core.run(upload_changesets.and_then(|()| {
        info!(logger, "finished uploading changesets, now doing bookmarks");
        upload_bookmarks
    })).expect("main stream failed");
}
