/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use cacheblob::{dummy::DummyLease, LeaseOps};
use clap::{App, Arg, ArgMatches};
use cmdlib::args;
use context::CoreContext;
use derived_data_utils::{derived_data_utils, BackfillDeriveStats, DerivedUtils};
use futures::{stream, StreamExt, TryStreamExt};
use futures_stats::TimedTryFutureExt;
use mononoke_types::ChangesetId;
use serde::ser::SerializeStruct;
use slog::debug;
use std::{sync::Arc, time::Duration};

const ARG_BACKFILL: &str = "backfill";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_PARALLEL: &str = "parallel";

pub enum DeriveOptions {
    // Simple case - derive commits one by one
    Simple,
    // Derive commits one by one, but send all writes
    // to in-memory blobstore first, and only when
    // all commits were derived send them to real blobstore.
    Backfill,
    // Use backfill mode, but also use parallel derivation -
    // some derived data types (e.g. fsnodes, skeleton manifests)
    // can derive the whole stack of commits in parallel.
    BackfillParallel { batch_size: Option<u64> },
}

impl DeriveOptions {
    pub fn add_opts<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
        subcommand
            .arg(
                Arg::with_name(ARG_BACKFILL)
                    .long(ARG_BACKFILL)
                    .required(false)
                    .takes_value(false)
                    .help("Whether we need to use backfill mode"),
            )
            .arg(
                Arg::with_name(ARG_PARALLEL)
                    .long(ARG_PARALLEL)
                    .required(false)
                    .takes_value(false)
                    .requires(ARG_BACKFILL)
                    .help("Whether we need to us parallel mode"),
            )
            .arg(
                Arg::with_name(ARG_BATCH_SIZE)
                    .long(ARG_BATCH_SIZE)
                    .required(false)
                    .takes_value(true)
                    .requires(ARG_PARALLEL)
                    .help("size of batch that will be derived in parallel"),
            )
    }

    pub fn from_matches(matches: &ArgMatches<'_>) -> Result<DeriveOptions, Error> {
        let opts = if matches.is_present(ARG_BACKFILL) {
            if matches.is_present(ARG_PARALLEL) {
                let batch_size = args::get_u64_opt(&matches, ARG_BATCH_SIZE);

                DeriveOptions::BackfillParallel { batch_size }
            } else {
                DeriveOptions::Backfill
            }
        } else {
            DeriveOptions::Simple
        };

        Ok(opts)
    }
}

pub enum BenchmarkResult {
    Simple {
        // Time it took to derive all changesets
        total_time: Duration,
        // How long it took to derive a given commit including saving data
        // to blobstore
        per_commit_stats: Vec<(ChangesetId, Duration)>,
    },
    Backfill {
        // Time it took to derive all changesets
        total_time: Duration,
        // How long it took to derive a given commit.
        // NOTE: Since backfilling mode is used this time DOES NOT
        // include the time it took to save data to blobstore.
        per_commit_stats: Vec<(ChangesetId, Duration)>,
    },
    BackfillParallel {
        // Time it took to derive all changesets
        total_time: Duration,
    },
}

pub async fn regenerate_derived_data(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_type: String,
    opts: &DeriveOptions,
) -> Result<BenchmarkResult, Error> {
    let repo = repo.dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>);

    let derived_utils = derived_data_utils(ctx.fb, &repo, derived_data_type)?;
    // For benchmark we want all commits to be derived already - in that case we know that
    // we can use `backfill_batch_dangerous()` function (because all dependent derive data
    // types are derived) and also we know that all ancestors of `csids` are derived.
    let pending = derived_utils
        .pending(ctx.clone(), repo.clone(), csids.clone())
        .await?;
    if !pending.is_empty() {
        return Err(anyhow!(
            "{} commits are not derived yet. \
        Regenerating requires all commits to be derived. List of underived commits: {:?}",
            pending.len(),
            pending
        ));
    }
    let csids = topo_sort(&ctx, &repo, csids).await?;

    match opts {
        DeriveOptions::Simple => derive_simple(&ctx, &repo, csids, derived_utils).await,
        DeriveOptions::Backfill => derive_with_backfill(&ctx, &repo, csids, derived_utils).await,
        DeriveOptions::BackfillParallel { batch_size } => {
            derive_with_parallel_backfill(&ctx, &repo, csids, derived_utils, *batch_size).await
        }
    }
}

async fn derive_simple(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_utils: Arc<dyn DerivedUtils>,
) -> Result<BenchmarkResult, Error> {
    derived_data_utils.regenerate(&csids);
    let (stats, per_commit_stats) = async {
        let mut per_commit_stats = vec![];
        for csid in csids {
            let (stats, _) = derived_data_utils
                .derive(ctx.clone(), repo.clone(), csid)
                .try_timed()
                .await?;
            per_commit_stats.push((csid, stats.completion_time));
        }
        Result::<_, Error>::Ok(per_commit_stats)
    }
    .try_timed()
    .await?;

    Ok(BenchmarkResult::Simple {
        total_time: stats.completion_time,
        per_commit_stats,
    })
}

async fn derive_with_backfill(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_utils: Arc<dyn DerivedUtils>,
) -> Result<BenchmarkResult, Error> {
    derived_data_utils.regenerate(&csids);
    let (stats, backfill_derive_stats) = derived_data_utils
        .backfill_batch_dangerous(
            ctx.clone(),
            repo.clone(),
            csids,
            false, /* parallel */
            None,
        )
        .try_timed()
        .await?;

    let mut all_per_commit_stats = vec![];
    if let BackfillDeriveStats::Serial(per_commit_stats) = backfill_derive_stats {
        let per_commit_stats = per_commit_stats
            .into_iter()
            .map(|(cs_id, stat)| (cs_id, stat))
            .collect::<Vec<_>>();
        all_per_commit_stats.extend(per_commit_stats);
    }

    Ok(BenchmarkResult::Backfill {
        total_time: stats.completion_time,
        per_commit_stats: all_per_commit_stats,
    })
}

async fn derive_with_parallel_backfill(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_utils: Arc<dyn DerivedUtils>,
    batch_size: Option<u64>,
) -> Result<BenchmarkResult, Error> {
    let batch_size = batch_size.unwrap_or(csids.len() as u64);

    let (stats, ()) = async {
        for chunk in csids.chunks(batch_size as usize) {
            derived_data_utils.regenerate(&chunk.to_vec());
            derived_data_utils
                .backfill_batch_dangerous(
                    ctx.clone(),
                    repo.clone(),
                    chunk.to_vec(),
                    true, /* parallel */
                    None,
                )
                .await?;
            derived_data_utils.clear_regenerate();
        }
        Result::<_, Error>::Ok(())
    }
    .try_timed()
    .await?;

    Ok(BenchmarkResult::BackfillParallel {
        total_time: stats.completion_time,
    })
}

async fn topo_sort(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
) -> Result<Vec<ChangesetId>, Error> {
    debug!(ctx.logger(), "Toposorting");
    let cs_fetcher = &repo.get_changeset_fetcher();
    let mut csids_with_gen_num = stream::iter(csids)
        .map(|cs_id| async move {
            let gen_num = cs_fetcher.get_generation_number(ctx.clone(), cs_id).await?;
            Result::<_, Error>::Ok((cs_id, gen_num))
        })
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;
    csids_with_gen_num.sort_by_key(|(_, gen)| *gen);

    Ok(csids_with_gen_num
        .into_iter()
        .map(|(csid, _)| csid)
        .collect())
}

impl serde::Serialize for BenchmarkResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use BenchmarkResult::*;
        match self {
            Simple {
                total_time,
                per_commit_stats,
            } => {
                let mut s = serializer.serialize_struct("BenchmarkResult", 3)?;
                s.serialize_field("type", "simple")?;
                s.serialize_field("total_time", total_time)?;
                s.serialize_field("per_commit_stats", per_commit_stats)?;
                s.end()
            }
            Backfill {
                total_time,
                per_commit_stats,
            } => {
                let mut s = serializer.serialize_struct("BenchmarkResult", 3)?;
                s.serialize_field("type", "backfill")?;
                s.serialize_field("total_time", total_time)?;
                s.serialize_field("per_commit_stats", per_commit_stats)?;
                s.end()
            }
            BackfillParallel { total_time } => {
                let mut s = serializer.serialize_struct("BenchmarkResult", 2)?;
                s.serialize_field("type", "backfill_parallel")?;
                s.serialize_field("total_time", total_time)?;
                s.end()
            }
        }
    }
}

pub fn print_benchmark_result(res: &BenchmarkResult, json: bool) -> Result<(), Error> {
    use BenchmarkResult::*;

    if json {
        let s = serde_json::to_string_pretty(res)?;
        println!("{}", s);
    } else {
        match res {
            Simple {
                total_time,
                per_commit_stats,
            } => {
                println!("Total time: {}ms", total_time.as_millis());
                for (cs_id, time) in per_commit_stats {
                    println!("{}: {}ms", cs_id, time.as_millis());
                }
            }
            Backfill {
                total_time,
                per_commit_stats,
            } => {
                println!("Total time: {}ms", total_time.as_millis());
                for (cs_id, time) in per_commit_stats {
                    println!("{}: {}ms", cs_id, time.as_millis());
                }
            }
            BackfillParallel { total_time } => {
                println!("Total time: {}ms", total_time.as_millis());
            }
        }
    }

    Ok(())
}
