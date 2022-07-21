/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod config;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use blobrepo_utils::BonsaiMFVerify;
use blobrepo_utils::BonsaiMFVerifyResult;
use blobstore::Loadable;
use clap::Parser;
use clap::Subcommand;
use cloned::cloned;
use context::CoreContext;
use failure_ext::DisplayChain;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_old::future::Either;
use futures_old::future::{self as old_future};
use futures_old::Future;
use futures_old::Stream;
use lock_ext::LockExt;
use mercurial_derived_data::get_manifest_from_bonsai;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeAppBuilder;
use revset::AncestorsNodeStream;
use slog::debug;
use slog::error;
use slog::info;
use slog::warn;
use slog::Logger;
use std::collections::HashSet;
use std::io::Write;
use std::process;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Parser)]
struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcmd: BonsaiSubCommand,
}

#[derive(Subcommand)]
enum BonsaiSubCommand {
    RoundTrip(RoundTrip),
    HgManifest(HgManifest),
}

/// Verify that bonsai changesets roundtrip correctly
#[derive(Parser)]
struct RoundTrip {
    /// How many changesets to follow before stopping
    #[clap(long, default_value_t = 1024)]
    limit: usize,

    /// Print list of changed entries between manifests
    #[clap(long)]
    changes: bool,

    /// Debug mode
    #[clap(long)]
    debug: bool,

    /// Configuration file, see source code for spec
    #[clap(long, value_name = "TOML")]
    config: Option<String>,

    /// Changesets from which to start traversing
    #[clap(required = true, parse(try_from_str))]
    start_points: Vec<HgChangesetId>,
}

/// Verify generation of various things
#[derive(Parser)]
struct HgManifest {
    /// Starting point of traversal
    #[clap(parse(try_from_str))]
    hg_changeset_id: HgChangesetId,

    /// Count of changeset to traverse
    count: usize,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb).build::<CommandArgs>()?;
    let args: CommandArgs = app.args()?;
    let runtime = app.runtime();
    let repo = runtime.block_on(app.open_repo(&args.repo))?;
    let logger = app.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match args.subcmd {
        BonsaiSubCommand::RoundTrip(args) => {
            subcommand_round_trip(ctx, logger.clone(), runtime, repo, args)
        }
        BonsaiSubCommand::HgManifest(args) => {
            subcommmand_hg_manifest_verify(&ctx, runtime, &repo, args)
        }
    }
}

fn subcommand_round_trip(
    ctx: CoreContext,
    logger: Logger,
    runtime: &tokio::runtime::Handle,
    repo: BlobRepo,
    args: RoundTrip,
) -> Result<()> {
    let config = config::get_config(args.config).expect("getting configuration failed");
    let start_points = args.start_points;
    let follow_limit = args.limit;
    let print_changes = args.changes;
    let debug_bonsai_diff = args.debug && args.changes;

    let valid = Arc::new(AtomicUsize::new(0));
    let invalid = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let ignored = Arc::new(AtomicUsize::new(0));
    // The number of changesets at the end should be really small, so an unbounded buffer doesn't
    // matter much.
    let (end_sender, end_receiver) = ::std::sync::mpsc::channel();

    let verify_fut = old_future::lazy(|| {
        let logger = logger.clone();
        let valid = valid.clone();
        let invalid = invalid.clone();
        let errors = errors.clone();
        let ignored = ignored.clone();
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
                    let logger =
                        logger.new(slog::o!["changeset_id" => format!("{}", meta.changeset_id)]);

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
                            Either::A(old_future::ok(()))
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
                            Either::A(old_future::ok(()))
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
                                        info!(logger, "Change: {:?}", changed_entry,);
                                    })
                                    .collect()
                                    .map(|_| ());
                                Either::B(diff_fut)
                            } else {
                                Either::A(old_future::ok(()))
                            }
                        }
                        BonsaiMFVerifyResult::Ignored(..) => {
                            ignored.fetch_add(1, Ordering::Relaxed);
                            Either::A(old_future::ok(()))
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
    });

    let _ = runtime.block_on(verify_fut.compat());

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
    ctx: &CoreContext,
    runtime: &tokio::runtime::Handle,
    repo: &BlobRepo,
    args: HgManifest,
) -> Result<()> {
    let total = &AtomicUsize::new(0);
    let total_millis = &AtomicU64::new(0);
    let bad = &Mutex::new(HashSet::new());

    let run = async move {
        let count = args.count;
        let hg_csid = args.hg_changeset_id;
        let csid = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, hg_csid)
            .await?
            .ok_or_else(|| format_err!("failed to fetch bonsai changeset"))?;

        AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), csid)
            .compat()
            .take(count)
            .map(|res| async move {
                match res {
                    Ok(csid) => {
                        let cs_id = repo.derive_hg_changeset(ctx, csid).await?;
                        let bonsai_fut = csid.load(ctx, repo.blobstore()).map_err(Error::from);

                        let parents_fut = async move {
                            let blob_cs = cs_id.load(ctx, repo.blobstore()).await?;
                            let expected = blob_cs.manifestid();

                            let hg_csid = blob_cs.get_changeset_id();

                            let mut parent_hashes = vec![];
                            parent_hashes.extend(blob_cs.p1());
                            parent_hashes.extend(blob_cs.p2());
                            parent_hashes.extend(blob_cs.step_parents()?);

                            let parents = future::try_join_all(
                                parent_hashes
                                    .into_iter()
                                    .map(|p| {
                                        cloned!(ctx, repo);
                                        let cs_id = HgChangesetId::new(p);
                                        async move {
                                            cs_id
                                                .load(&ctx, repo.blobstore())
                                                .map_ok(|cs| cs.manifestid())
                                                .map_err(Error::from)
                                                .await
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .await?;

                            Ok((parents, hg_csid, expected))
                        };

                        let (bonsai, (parents, hg_csid, expected)) =
                            future::try_join(bonsai_fut, parents_fut).await?;

                        let start = Instant::now();

                        get_manifest_from_bonsai(
                            ctx.clone(),
                            repo.get_blobstore().boxed(),
                            bonsai.clone(),
                            parents,
                        )
                        .map_ok(move |result| {
                            if result != expected {
                                println!(
                                    "\x1b[KBAD hg_cisd:{} result:{} expected:{}\x1b[m",
                                    hg_csid, result, expected,
                                );
                                bad.with(|bad| bad.insert((csid, hg_csid, result, expected)));
                            }

                            let all = total.fetch_add(1, Ordering::SeqCst) + 1;
                            total_millis
                                .fetch_add(start.elapsed().as_millis() as u64, Ordering::SeqCst);
                            print!(
                                "\x1b[K {} total:{} bad:{} mean_time:{:.2} ms \r",
                                hg_csid,
                                all,
                                bad.with(|bad| bad.len()),
                                total_millis.load(Ordering::SeqCst) as f32 / all as f32,
                            );
                            std::io::stdout().flush().expect("flush on stdout failed");
                        })
                        .await
                    }
                    Err(e) => Err(e),
                }
            })
            .buffer_unordered(100)
            .try_for_each(|_| async { Ok(()) })
            .map_ok(move |_| {
                let bad = bad.with(|bad| std::mem::take(bad));
                if bad.is_empty() {
                    println!()
                } else {
                    println!("\n BAD: {:#?}", bad)
                }
            })
            .await
    };

    runtime.block_on(run)
}
