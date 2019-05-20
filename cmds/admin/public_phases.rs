// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::ArgMatches;
use cloned::cloned;
use failure_ext::Error;
use futures::future::ok;
use futures::prelude::*;
use futures::stream::iter_ok;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use blobrepo::BlobRepo;
use cmdlib::args;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use phases::{Phase, Phases, SqlPhases};
use slog::{info, Logger};

fn add_public_phases(
    ctx: CoreContext,
    repo: BlobRepo,
    phases: Arc<SqlPhases>,
    logger: Logger,
    path: impl AsRef<str>,
    chunk_size: usize,
) -> BoxFuture<(), Error> {
    let file = try_boxfuture!(File::open(path.as_ref()).map_err(Error::from));
    let hg_changesets = BufReader::new(file).lines().filter_map(|id_str| {
        id_str
            .map_err(Error::from)
            .and_then(|v| HgChangesetId::from_str(&v))
            .ok()
    });
    let entries_processed = Arc::new(AtomicUsize::new(0));
    info!(logger, "start processing hashes");
    iter_ok(hg_changesets)
        .chunks(chunk_size)
        .and_then(move |chunk| {
            let count = chunk.len();
            repo.get_hg_bonsai_mapping(ctx.clone(), chunk)
                .map(|changesets| {
                    changesets
                        .into_iter()
                        .map(|(_, cs)| (cs, Phase::Public))
                        .collect()
                })
                .and_then({
                    cloned!(ctx, repo, phases);
                    move |phases_mapping| phases.add_all(ctx, repo, phases_mapping)
                })
                .and_then({
                    cloned!(entries_processed);
                    move |_| {
                        print!(
                            "\x1b[Khashes processed: {}\r",
                            entries_processed.fetch_add(count, Ordering::SeqCst) + count,
                        );
                        std::io::stdout().flush().expect("flush on stdout failed");
                        tokio_timer::sleep(Duration::from_secs(5)).map_err(Error::from)
                    }
                })
        })
        .for_each(|_| ok(()))
        .boxify()
}

pub fn subcommand_add_public_phases(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let path = String::from(sub_m.value_of("input-file").unwrap());
    let chunk_size = sub_m
        .value_of("chunk-size")
        .and_then(|chunk_size| chunk_size.parse::<usize>().ok())
        .unwrap_or(16384);
    let ctx = CoreContext::test_mock();
    args::init_cachelib(&matches);
    let phases: Arc<_> =
        Arc::new(args::open_sql::<SqlPhases>(&matches).expect("Failed to open the db with phases"));
    args::open_repo(&logger, &matches)
        .and_then(move |repo| add_public_phases(ctx, repo, phases, logger, path, chunk_size))
        .boxify()
}
