/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use blobrepo::BlobRepo;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use bulkops::MAX_FETCH_STEP;
use changesets::ChangesetsArc;
use clap::Arg;
use cmdlib::args;
use context::CoreContext;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use phases::PhasesArc;
use tokio::runtime::Handle;

const BENCHMARK_SAVE_BASELINE_ARG: &str = "benchmark-save-baseline";
const BENCHMARK_USE_BASELINE_ARG: &str = "benchmark-use-baseline";
const BENCHMARK_FILTER_ARG: &str = "benchmark-filter";

pub fn bench_stream<'a, F, S, O, E>(
    c: &'a mut Criterion,
    ctx: &'a CoreContext,
    runtime: &Handle,
    group: String,
    fetcher: &'a PublicChangesetBulkFetch,
    to_stream: F,
) where
    F: Fn(&'a CoreContext, &'a PublicChangesetBulkFetch) -> S,
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
fn main(fb: fbinit::FacebookInit) {
    let app = args::MononokeAppBuilder::new("benchmark_bulkops")
         .with_advanced_args_hidden()
         .build()
         .arg(
             Arg::with_name(BENCHMARK_SAVE_BASELINE_ARG)
                 .long(BENCHMARK_SAVE_BASELINE_ARG)
                 .takes_value(true)
                 .required(false)
                 .help("save results as a baseline under given name, for comparison"),
         )
         .arg(
             Arg::with_name(BENCHMARK_USE_BASELINE_ARG)
                 .long(BENCHMARK_USE_BASELINE_ARG)
                 .takes_value(true)
                 .required(false)
                 .conflicts_with(BENCHMARK_SAVE_BASELINE_ARG)
                 .help("compare to named baseline instead of last run"),
         )
         .arg(
             Arg::with_name(BENCHMARK_FILTER_ARG)
                 .long(BENCHMARK_FILTER_ARG)
                 .takes_value(true)
                 .required(false)
                 .multiple(true)
                 .help("limit to benchmarks whose name contains this string. Repetition tightens the filter"),
         );
    let matches = app.get_matches(fb).expect("Failed to start Mononoke");

    let mut criterion = Criterion::default()
        .measurement_time(Duration::from_secs(450))
        .sample_size(10)
        .warm_up_time(Duration::from_secs(60));

    if let Some(baseline) = matches.value_of(BENCHMARK_SAVE_BASELINE_ARG) {
        criterion = criterion.save_baseline(baseline.to_string());
    }
    if let Some(baseline) = matches.value_of(BENCHMARK_USE_BASELINE_ARG) {
        criterion = criterion.retain_baseline(baseline.to_string());
    }

    if let Some(filters) = matches.values_of(BENCHMARK_FILTER_ARG) {
        for filter in filters {
            criterion = criterion.with_filter(filter.to_string())
        }
    }

    let logger = matches.logger();
    let runtime = matches.runtime();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let blobrepo = args::not_shardmanager_compatible::open_repo::<BlobRepo>(fb, logger, &matches);

    let setup = {
        |runtime: &Handle| {
            runtime.block_on(async move {
                let blobrepo = blobrepo.await.expect("blobrepo should open");
                (
                    blobrepo.name().to_string(),
                    PublicChangesetBulkFetch::new(blobrepo.changesets_arc(), blobrepo.phases_arc()),
                )
            })
        }
    };

    // Tests are run from here
    let (repo, fetcher) = setup(runtime);

    bench_stream(
        &mut criterion,
        &ctx,
        runtime,
        format!(
            "{}{}",
            repo, ":PublicChangesetBulkFetch::fetch_best_newest_first_mid"
        ),
        &fetcher,
        |ctx, fetcher| {
            async move {
                let (lower, upper) = fetcher.get_repo_bounds(ctx).await?;
                let mid = (upper - lower) / 2;
                Ok(fetcher.fetch_ids(ctx, Direction::NewestFirst, Some((lower, mid))))
            }
            .try_flatten_stream()
        },
    );

    bench_stream(
        &mut criterion,
        &ctx,
        runtime,
        format!(
            "{}{}",
            repo, ":PublicChangesetBulkFetch::fetch_best_oldest_first"
        ),
        &fetcher,
        |ctx, fetcher| fetcher.fetch_ids(ctx, Direction::OldestFirst, None),
    );

    bench_stream(
        &mut criterion,
        &ctx,
        runtime,
        format!(
            "{}{}",
            repo, ":PublicChangesetBulkFetch::fetch_entries_oldest_first"
        ),
        &fetcher,
        |ctx, fetcher| fetcher.fetch(ctx, Direction::OldestFirst),
    );

    criterion.final_summary();
}
