/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod config;

use blobrepo_utils::{BonsaiMFVerify, BonsaiMFVerifyResult};
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers::create_runtime};
use context::CoreContext;
use failure_ext::{err_msg, format_err, DisplayChain, Result};
use fbinit::FacebookInit;
use futures::{
    future::{self, Either},
    Future, IntoFuture, Stream,
};
use futures_ext::FutureExt;
use lock_ext::LockExt;
use mercurial_types::{Changeset, HgChangesetId};
use revset::AncestorsNodeStream;
use slog::{debug, error, info, warn, Logger};
use std::{
    collections::HashSet,
    io::Write,
    process, result,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("bonsai roundtrip verification")
        .build()
        .version("0.0.0")
        .subcommand(
            SubCommand::with_name("round-trip")
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
                ),
        )
        .subcommand(
            SubCommand::with_name("hg-manifest")
                .about("verify generation of various things")
                .arg(
                    Arg::with_name("hg-changeset-id")
                        .help("starting point of traversal")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("count")
                        .help("count of changset to traverse")
                        .required(true)
                        .index(2),
                ),
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

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches();
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match matches.subcommand() {
        ("round-trip", Some(sub_m)) => subcommand_round_trip(ctx, logger, &matches, sub_m),
        ("hg-manifest", Some(sub_m)) => {
            subcommmand_hg_manifest_verify(ctx, logger, &matches, sub_m)
        }
        (subcommand, _) => Err(format_err!("unhandled subcommand {}", subcommand)),
    }
}

fn subcommand_round_trip(
    ctx: CoreContext,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Result<()> {
    args::init_cachelib(ctx.fb, &matches);
    let repo = args::open_repo(ctx.fb, &logger, &matches);

    let config = config::get_config(&matches).expect("getting configuration failed");
    let start_points = get_start_points(&sub_m);
    let follow_limit = args::get_usize(&sub_m, "limit", 1024);
    let print_changes = sub_m.is_present("changes");
    let debug_bonsai_diff = matches.is_present("debug") && sub_m.is_present("changes");

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
                    let logger = logger.new(slog::o!["changeset_id" => format!("{}", meta.changeset_id)]);

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

    let logger = logger.new(slog::o!["summary" => ""]);

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

fn subcommmand_hg_manifest_verify(
    ctx: CoreContext,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Result<()> {
    let hg_csid = sub_m
        .value_of("hg-changeset-id")
        .ok_or(err_msg("required parameter `hg-changeset-id` is not set"))
        .and_then(HgChangesetId::from_str);

    let count: Result<u64> = sub_m
        .value_of("count")
        .ok_or(err_msg("required parameter `count` is not set"))
        .and_then(|count_str| Ok(count_str.parse()?));

    args::init_cachelib(ctx.fb, &matches);

    let total = Arc::new(AtomicUsize::new(0));
    let total_millis = Arc::new(AtomicU64::new(0));
    let bad = Arc::new(Mutex::new(HashSet::new()));
    let run = (args::open_repo(ctx.fb, &logger, &matches), hg_csid, count)
        .into_future()
        .and_then({
            cloned!(ctx);
            move |(repo, hg_csid, count)| {
                repo.get_bonsai_from_hg(ctx, hg_csid)
                    .and_then(move |csid| match csid {
                        None => Err(err_msg("failed to fetch bonsai changeset")),
                        Some(csid) => Ok((repo, csid, count)),
                    })
            }
        })
        .and_then(move |(repo, csid, count)| {
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), csid)
                .take(count)
                .map({
                    cloned!(ctx, repo, bad, total, total_millis);
                    move |csid| {
                        (
                            repo.get_bonsai_changeset(ctx.clone(), csid),
                            repo.get_hg_from_bonsai_changeset(ctx.clone(), csid)
                                .and_then({
                                    cloned!(ctx, repo);
                                    move |hg_csid| repo.get_changeset_by_changesetid(ctx, hg_csid)
                                })
                                .and_then({
                                    cloned!(ctx, repo);
                                    move |hgchangeset| {
                                        let hg_csid = hgchangeset.get_changeset_id();
                                        let expected = hgchangeset.manifestid();
                                        let (p1, p2) = hgchangeset.parents().get_nodes();

                                        let p1_fetch = match p1 {
                                            None => future::ok(None).left_future(),
                                            Some(p1) => repo
                                                .get_changeset_by_changesetid(
                                                    ctx.clone(),
                                                    HgChangesetId::new(p1),
                                                )
                                                .map(|cs| Some(cs.manifestid()))
                                                .right_future(),
                                        };

                                        let p2_fetch = match p2 {
                                            None => future::ok(None).left_future(),
                                            Some(p2) => repo
                                                .get_changeset_by_changesetid(
                                                    ctx.clone(),
                                                    HgChangesetId::new(p2),
                                                )
                                                .map(|cs| Some(cs.manifestid()))
                                                .right_future(),
                                        };

                                        p1_fetch
                                            .join(p2_fetch)
                                            .map(move |(p1, p2)| (hg_csid, expected, p1, p2))
                                    }
                                }),
                        )
                            .into_future()
                            .and_then({
                                cloned!(ctx, repo, bad, total, total_millis);
                                move |(bonsai, (hg_csid, expected, p1, p2))| {
                                    let start = Instant::now();
                                    repo.get_manifest_from_bonsai(
                                        ctx.clone(),
                                        bonsai.clone(),
                                        p1,
                                        p2,
                                    )
                                    .map(move |(result, _)| {
                                        if result != expected {
                                            println!(
                                                "\x1b[KBAD hg_cisd:{} result:{} expected:{}\x1b[m",
                                                hg_csid, result, expected,
                                            );
                                            bad.with(|bad| {
                                                bad.insert((csid, hg_csid, result, expected))
                                            });
                                        }

                                        let all = total.fetch_add(1, Ordering::SeqCst) + 1;
                                        total_millis.fetch_add(
                                            start.elapsed().as_millis() as u64,
                                            Ordering::SeqCst,
                                        );
                                        print!(
                                            "\x1b[K {} total:{} bad:{} mean_time:{:.2} ms \r",
                                            hg_csid,
                                            all,
                                            bad.with(|bad| bad.len()),
                                            total_millis.load(Ordering::SeqCst) as f32 / all as f32,
                                        );
                                        std::io::stdout().flush().expect("flush on stdout failed");
                                    })
                                }
                            })
                    }
                })
                .buffer_unordered(100)
                .for_each(|_| Ok(()))
                .map(move |_| {
                    let bad = bad.with(|bad| std::mem::replace(bad, HashSet::new()));
                    if bad.is_empty() {
                        println!("")
                    } else {
                        println!("\n BAD: {:#?}", bad)
                    }
                })
        });

    let mut runtime = create_runtime(None)?;
    runtime.block_on(run)
}
