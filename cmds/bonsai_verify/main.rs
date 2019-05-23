// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate clap;
#[macro_use]
extern crate cloned;
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate tokio;
extern crate toml;

extern crate futures_ext;

extern crate blobrepo_utils;
extern crate cmdlib;
extern crate context;
extern crate failure_ext;
extern crate mercurial_types;
extern crate mononoke_types;

mod config;

use std::process;
use std::result;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::failure::DisplayChain;
use clap::{App, Arg, ArgMatches};
use futures::future::{self, Either};
use futures::prelude::*;
use slog::Logger;

use futures_ext::FutureExt;

use blobrepo_utils::{BonsaiMFVerify, BonsaiMFVerifyResult};
use cmdlib::args;
use context::CoreContext;
use mercurial_types::HgChangesetId;

use failure_ext::Result;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
        safe_writes: false,
        local_instances: true,
        default_glog: false,
    };
    app.build("bonsai roundtrip verification")
        .version("0.0.0")
        .about("Verify that bonsai changesets roundtrip correctly.")
        .args_from_usage(
            r#"
            --limit [LIMIT] 'how many changesets to follow before stopping [default: 1024]'
            --changes       'print list of changed entries between manifests'
            --config [TOML] 'configuration file, see source code for spec'
        "#,
        )
        .arg(
            Arg::with_name("start-points")
                .takes_value(true)
                .multiple(true)
                .required(true)
                .help("changesets from which to start traversing"),
        )
}

fn get_start_points<'a>(matches: &ArgMatches<'a>) -> Vec<HgChangesetId> {
    let res: result::Result<_, _> = matches
        .values_of("start-points")
        .expect("at least one start point must be specified")
        .map(|hash| hash.parse::<HgChangesetId>())
        .collect();

    res.expect("failed to parse start points as hashes")
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();
    let logger = args::get_logger(&matches);
    args::init_cachelib(&matches);

    // TODO(luk): This is not a test use case, fix it in next diffs
    let ctx = CoreContext::test_mock();
    let repo = args::open_repo(&logger, &matches);

    let config = config::get_config(&matches).expect("getting configuration failed");
    let start_points = get_start_points(&matches);
    let follow_limit = args::get_usize(&matches, "limit", 1024);
    let print_changes = matches.is_present("changes");
    let debug_bonsai_diff = matches.is_present("debug") && matches.is_present("changes");

    let valid = Arc::new(AtomicUsize::new(0));
    let invalid = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let ignored = Arc::new(AtomicUsize::new(0));
    // The number of changesets at the end should be really small, so an unbounded buffer doesn't
    // matter much.
    let (end_sender, end_receiver) = ::std::sync::mpsc::channel();

    let verify_fut = {
        let logger = logger.clone();
        let valid = valid.clone();
        let invalid = invalid.clone();
        let errors = errors.clone();
        let ignored = ignored.clone();
        repo
            .map_err({
                let logger = logger.clone();
                move |err| { error!(logger, "ERROR: Failed to create repo: {}", err); }
            })
            .and_then(move |repo| {
            let bonsai_verify = BonsaiMFVerify {
                ctx: ctx.clone(),
                logger: logger.clone(),
                repo,
                follow_limit,
                ignores: config.ignores.into_iter().collect(),
                broken_merges_before: config.broken_merges_before,
                debug_bonsai_diff,
            };
            bonsai_verify
            .verify(start_points)
            .and_then({
                cloned!(ctx, logger);
                move |(result, meta)| {
                    let logger = logger.new(o!["changeset_id" => format!("{}", meta.changeset_id)]);

                    if !result.is_ignored() {
                        let followed = follow_limit - meta.follow_remaining;
                        if followed % 10000 == 0 {
                            info!(
                                logger,
                                "Followed {} changesets, {} remaining",
                                followed,
                                meta.follow_remaining,
                            );
                        }
                        if meta.follow_remaining == 0 {
                            end_sender
                                .send(meta.changeset_id)
                                .expect("end_receiver is still alive");
                        }
                    }

                    let fut = match &result {
                        BonsaiMFVerifyResult::Valid { .. } => {
                            debug!(logger, "VALID");
                            valid.fetch_add(1, Ordering::Relaxed);
                            Either::A(future::ok(()))
                        }
                        BonsaiMFVerifyResult::ValidDifferentId(difference) => {
                            debug!(
                                logger,
                                "VALID but with a different hash: \
                                expected manifest ID: {}, roundtrip ID: {}",
                                difference.expected_mf_id,
                                difference.roundtrip_mf_id,
                            );
                            valid.fetch_add(1, Ordering::Relaxed);
                            Either::A(future::ok(()))
                        }
                        BonsaiMFVerifyResult::Invalid(difference) => {
                            warn!(logger, "INVALID");
                            info!(
                                logger, "manifest hash differs";
                                "expected manifest ID" => difference.expected_mf_id,
                                "roundtrip ID" => difference.roundtrip_mf_id,
                            );
                            invalid.fetch_add(1, Ordering::Relaxed);
                            if print_changes {
                                let logger = logger.clone();
                                let diff_fut = difference
                                    .changes(ctx.clone())
                                    .map(move |changed_entry| {
                                        info!(
                                            logger,
                                            "Change: {}",
                                            changed_entry,
                                        );
                                    })
                                    .collect()
                                    .map(|_| ());
                                Either::B(diff_fut)
                            } else {
                                Either::A(future::ok(()))
                            }
                        }
                        BonsaiMFVerifyResult::Ignored(..) => {
                            ignored.fetch_add(1, Ordering::Relaxed);
                            Either::A(future::ok(()))
                        }
                    };

                    fut
                }
            })
            .then(move |res| {
                // collect() below will stop after the first error, but we care about all errors.
                // So report them now and keep returning Ok.
                if let Err(err) = &res {
                    error!(logger, "ERROR: {}", DisplayChain::from(err));
                    errors.fetch_add(1, Ordering::Relaxed);
                }
                Ok::<_, ()>(())
            })
            // collect to turn the stream into a future that will finish when the stream is done
            .collect()
        })
        // discard to drop results since they've already been reported
        .discard()
    };

    tokio::run(verify_fut);

    let end_points: Vec<_> = end_receiver.into_iter().collect();
    process::exit(summarize(
        logger, end_points, valid, invalid, errors, ignored,
    ));
}

fn summarize(
    logger: Logger,
    end_points: Vec<HgChangesetId>,
    valid: Arc<AtomicUsize>,
    invalid: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
    ignored: Arc<AtomicUsize>,
) -> i32 {
    let end_points: Vec<_> = end_points
        .iter()
        .map(|changeset_id| format!("{}", changeset_id))
        .collect();
    let valid = valid.load(Ordering::Acquire);
    let invalid = invalid.load(Ordering::Acquire);
    let errors = errors.load(Ordering::Acquire);
    let ignored = ignored.load(Ordering::Acquire);
    let total = valid + invalid + errors;
    let percent_valid = 100.0 * (valid as f64) / (total as f64);

    let logger = logger.new(o!["summary" => ""]);

    info!(
        logger,
        "{:.2}% valid", percent_valid;
        "ignored" => ignored,
        "errors" => errors,
        "valid" => valid,
        "total" => total,
    );

    if !end_points.is_empty() {
        info!(
            logger,
            "To resume verification, run with arguments: {}",
            end_points.join(" "),
        );
    }

    // Return the appropriate exit code for this process.
    if errors > 0 {
        2
    } else if invalid > 0 {
        1
    } else {
        0
    }
}
