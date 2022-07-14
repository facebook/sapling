/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use cacheblob::dummy::DummyLease;
use cacheblob::LeaseOps;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use derived_data_utils::build_derive_graph;
use derived_data_utils::derived_data_utils;
use derived_data_utils::DeriveGraph;
use derived_data_utils::ThinOut;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use mononoke_types::ChangesetId;
use slog::debug;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

const ARG_BACKFILL: &str = "backfill";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_PARALLEL: &str = "parallel";

pub struct DeriveOptions {
    batch_size: u64,
    derivation_type: DerivationType,
}

#[derive(Eq, PartialEq)]
pub enum DerivationType {
    // Simple case - derive commits one by one
    Simple,
    // Derive commits one by one, but send all writes
    // to in-memory blobstore first, and only when
    // all commits were derived send them to real blobstore.
    Backfill,
    // Use backfill mode, but also use parallel derivation -
    // some derived data types (e.g. fsnodes, skeleton manifests)
    // can derive the whole stack of commits in parallel.
    BackfillParallel,
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
                    .help("size of batch that will be derived in parallel"),
            )
    }

    pub fn from_matches(matches: &ArgMatches<'_>) -> Result<DeriveOptions, Error> {
        let batch_size = args::get_u64(&matches, ARG_BATCH_SIZE, 20);
        let derivation_type = if matches.is_present(ARG_BACKFILL) {
            if matches.is_present(ARG_PARALLEL) {
                DerivationType::BackfillParallel
            } else {
                DerivationType::Backfill
            }
        } else {
            DerivationType::Simple
        };

        Ok(DeriveOptions {
            batch_size,
            derivation_type,
        })
    }
}

pub async fn regenerate_derived_data(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_types: Vec<String>,
    opts: &DeriveOptions,
) -> Result<RegenerateStats, Error> {
    let repo = repo.dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>);

    let mut derived_utils = vec![];
    for ty in derived_data_types {
        derived_utils.push(derived_data_utils(ctx.fb, &repo, ty)?);
    }

    for utils in &derived_utils {
        // For benchmark we want all commits to be derived already - in that case we know that
        // we can use `backfill_batch_dangerous()` function (because all dependent derive data
        // types are derived) and also we know that all ancestors of `csids` are derived.
        let pending = utils
            .pending(ctx.clone(), repo.clone(), csids.clone())
            .await?;
        if !pending.is_empty() {
            return Err(anyhow!(
                "{} commits are not derived yet. \
            Benchmarking requires all commits to be derived. List of underived commits: {:?}",
                pending.len(),
                pending
            ));
        }
    }

    let csids = topo_sort(ctx, &repo, csids).await?;
    for utils in &derived_utils {
        utils.regenerate(&csids);
    }

    let (stats, derive_graph) = build_derive_graph(
        ctx,
        &repo,
        csids,
        derived_utils.clone(),
        opts.batch_size as usize,
        ThinOut::new(1000.0, 1.5),
    )
    .try_timed()
    .await?;
    let build_derive_graph_duration = stats.completion_time;

    let start = Instant::now();
    match opts.derivation_type {
        DerivationType::Simple => {
            bounded_traversal::bounded_traversal_dag(
                100,
                derive_graph.clone(),
                |node| {
                    async move {
                        let deps = node.dependencies.clone();
                        Ok((node, deps))
                    }
                    .boxed()
                },
                {
                    cloned!(repo);
                    move |node: DeriveGraph, _| {
                        cloned!(ctx, repo);
                        async move {
                            if let Some(deriver) = &node.deriver {
                                for csid in &node.csids {
                                    deriver.derive(ctx.clone(), repo.clone(), *csid).await?;
                                }
                            }
                            Result::<_, Error>::Ok(())
                        }
                        .boxed()
                    }
                },
            )
            .await?;
        }
        DerivationType::Backfill | DerivationType::BackfillParallel => {
            let parallel = opts.derivation_type == DerivationType::BackfillParallel;
            derive_graph
                .derive(
                    ctx.clone(),
                    repo.clone(),
                    parallel,
                    None, /* gap size */
                )
                .await?;
        }
    };

    Ok(RegenerateStats {
        build_derive_graph: build_derive_graph_duration,
        derivation: start.elapsed(),
    })
}

pub struct RegenerateStats {
    pub build_derive_graph: Duration,
    pub derivation: Duration,
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
