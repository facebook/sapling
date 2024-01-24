/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use blobrepo::BlobRepo;
use bulkops::ChangesetBulkFetcher;
use bulkops::Direction;
use bulkops::MAX_FETCH_STEP;
use changesets::ChangesetsArc;
use clap::Parser;
use context::CoreContext;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeAppBuilder;
use phases::PhasesArc;
use repo_identity::RepoIdentityRef;
use tokio::runtime::Handle;

/// Benchmark bulkops.
#[derive(Parser)]
struct BenchmarkArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Save results as a baseline under this name, for comparison.
    #[clap(long)]
    save_baseline: Option<String>,

    /// Compare to named baseline instead of last run.
    #[clap(long)]
    use_baseline: Option<String>,

    /// Limit to benchmarks whose name contains this string. Repetition tightens the filter.
    #[clap(long)]
    filter: Vec<String>,
}

pub fn bench_stream<'a, F, S, O, E>(
    c: &'a mut Criterion,
    ctx: &'a CoreContext,
    runtime: &Handle,
    group: String,
    fetcher: &'a ChangesetBulkFetcher,
    to_stream: F,
) where
    F: Fn(&'a CoreContext, &'a ChangesetBulkFetcher) -> S,
    S: Stream<Item = Result<O, E>> + 'a,
    E: std::fmt::Debug,
{
    let mut group = c.benchmark_group(group);
    let num_to_load = 100000;
    group.throughput(Throughput::Elements(num_to_load));
    let step = MAX_FETCH_STEP;
    group.bench_with_input(
        BenchmarkId::from_parameter(step),
        &num_to_load,
        |b, &num_to_load| {
            let test = || async {
                let mut loaded: u64 = 0;
                let stream = to_stream(ctx, fetcher).take_while(|_entry| {
                    loaded += 1;
                    future::ready(loaded < num_to_load)
                });
                let _ = stream
                    .try_collect::<Vec<_>>()
                    .await
                    .expect("no stream errors");
            };
            b.iter(|| runtime.block_on(async { test().await }));
        },
    );
    group.finish();
}

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb).build::<BenchmarkArgs>()?;
    let args: BenchmarkArgs = app.args()?;

    let mut criterion = Criterion::default()
        .measurement_time(Duration::from_secs(450))
        .sample_size(10)
        .warm_up_time(Duration::from_secs(60));

    if let Some(baseline) = &args.save_baseline {
        criterion = criterion.save_baseline(baseline.to_string());
    }
    if let Some(baseline) = &args.use_baseline {
        criterion = criterion.retain_baseline(baseline.to_string());
    }

    for filter in args.filter.iter() {
        criterion = criterion.with_filter(filter.to_string())
    }

    let logger = app.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo = app.runtime().block_on(async {
        app.open_repo::<BlobRepo>(&args.repo)
            .await
            .context("Failed to open repo")
    })?;
    let fetcher = ChangesetBulkFetcher::new(repo.changesets_arc(), repo.phases_arc());
    let repo_name = repo.repo_identity().name().to_string();

    // Tests are run from here

    bench_stream(
        &mut criterion,
        &ctx,
        app.runtime(),
        format!(
            "{}{}",
            repo_name, ":ChangesetBulkFetcher::fetch_best_newest_first_mid"
        ),
        &fetcher,
        |ctx, fetcher| {
            async move {
                let (lower, upper) = fetcher.get_repo_bounds(ctx).await?;
                let mid = (upper - lower) / 2;
                Ok(fetcher.fetch_public_ids(ctx, Direction::NewestFirst, Some((lower, mid))))
            }
            .try_flatten_stream()
        },
    );

    bench_stream(
        &mut criterion,
        &ctx,
        app.runtime(),
        format!(
            "{}{}",
            repo_name, ":ChangesetBulkFetcher::fetch_best_oldest_first"
        ),
        &fetcher,
        |ctx, fetcher| fetcher.fetch_public_ids(ctx, Direction::OldestFirst, None),
    );

    bench_stream(
        &mut criterion,
        &ctx,
        app.runtime(),
        format!(
            "{}{}",
            repo_name, ":ChangesetBulkFetcher::fetch_entries_oldest_first"
        ),
        &fetcher,
        |ctx, fetcher| fetcher.fetch_public(ctx, Direction::OldestFirst),
    );

    criterion.final_summary();

    Ok(())
}
