/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::{sync::Arc, time::Duration};

use clap::Arg;
use criterion::Criterion;
use futures::compat::Future01CompatExt;
use tokio_compat::runtime::Runtime;

use blobrepo_factory::Caching;
use blobstore::Blobstore;
use blobstore_factory::make_blobstore;
use cacheblob::{new_cachelib_blobstore_no_lease, new_memcache_blobstore_no_lease};
use cmdlib::args;
use context::CoreContext;

mod parallel_puts;
mod single_puts;

mod parallel_different_blob_gets;
mod parallel_same_blob_gets;
mod single_gets;

pub const KB: usize = 1024;
pub const MB: usize = KB * 1024;
const ARG_STORAGE_CONFIG_NAME: &str = "storage-config-name";
const ARG_SAVE_BASELINE: &str = "save-baseline";
const ARG_USE_BASELINE: &str = "use-baseline";
const ARG_FILTER_BENCHMARKS: &str = "filter";

const BLOBSTORE_BLOBS_CACHE_POOL: &str = "blobstore-blobs";
const BLOBSTORE_PRESENCE_CACHE_POOL: &str = "blobstore-presence";

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) {
    let app = args::MononokeApp::new("benchmark_storage_config")
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .arg(
            Arg::with_name(ARG_STORAGE_CONFIG_NAME)
                .long(ARG_STORAGE_CONFIG_NAME)
                .takes_value(true)
                .required(true)
                .help("the name of the storage config to benchmark"),
        )
        .arg(
            Arg::with_name(ARG_SAVE_BASELINE)
                .long(ARG_SAVE_BASELINE)
                .takes_value(true)
                .required(false)
                .help("save results as a baseline under given name, for comparison"),
        )
        .arg(
            Arg::with_name(ARG_USE_BASELINE)
                .long(ARG_USE_BASELINE)
                .takes_value(true)
                .required(false)
                .conflicts_with(ARG_SAVE_BASELINE)
                .help("compare to named baseline instead of last run"),
        )
        .arg(
            Arg::with_name(ARG_FILTER_BENCHMARKS)
                .long(ARG_FILTER_BENCHMARKS)
                .takes_value(true)
                .required(false)
                .multiple(true)
                .help("limit to benchmarks whose name contains this string. Repetition tightens the filter"),
        );
    let matches = app.get_matches();

    let mut criterion = Criterion::default()
        .measurement_time(Duration::from_secs(60))
        .sample_size(10)
        .warm_up_time(Duration::from_secs(60));

    if let Some(baseline) = matches.value_of(ARG_SAVE_BASELINE) {
        criterion = criterion.save_baseline(baseline.to_string());
    }
    if let Some(baseline) = matches.value_of(ARG_USE_BASELINE) {
        criterion = criterion.retain_baseline(baseline.to_string());
    }

    if let Some(filters) = matches.values_of(ARG_FILTER_BENCHMARKS) {
        for filter in filters {
            criterion = criterion.with_filter(filter.to_string())
        }
    }

    let logger = args::init_logging(fb, &matches);
    let caching = args::init_cachelib(fb, &matches, None);

    let storage_config = args::read_storage_configs(fb, &matches)
        .expect("Could not read storage configs")
        .remove(
            matches
                .value_of(ARG_STORAGE_CONFIG_NAME)
                .expect("No storage config name"),
        )
        .expect("Storage config not found");
    let mysql_options = args::parse_mysql_options(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let mut runtime = args::init_runtime(&matches).expect("Cannot start tokio");
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let blobstore = runtime.block_on_std(async {
        let blobstore = make_blobstore(
            fb,
            storage_config.blobstore,
            mysql_options,
            blobstore_factory::ReadOnlyStorage(false),
            blobstore_options,
            logger,
        )
        .compat()
        .await
        .expect("Could not make blobstore");
        match caching {
            Caching::Disabled => blobstore,
            Caching::CachelibOnlyBlobstore => {
                let blob_pool = Arc::new(
                    cachelib::get_pool(BLOBSTORE_BLOBS_CACHE_POOL)
                        .expect("Could not get blob pool"),
                );
                let presence_pool = Arc::new(
                    cachelib::get_pool(BLOBSTORE_PRESENCE_CACHE_POOL)
                        .expect("Could not get presence pool"),
                );
                Arc::new(new_cachelib_blobstore_no_lease(
                    blobstore,
                    blob_pool,
                    presence_pool,
                ))
            }
            Caching::Enabled => {
                let blob_pool = Arc::new(
                    cachelib::get_pool(BLOBSTORE_BLOBS_CACHE_POOL)
                        .expect("Could not get blob pool"),
                );
                let presence_pool = Arc::new(
                    cachelib::get_pool(BLOBSTORE_PRESENCE_CACHE_POOL)
                        .expect("Could not get presence pool"),
                );
                let cachelib_blobstore =
                    new_cachelib_blobstore_no_lease(blobstore, blob_pool, presence_pool);
                Arc::new(
                    new_memcache_blobstore_no_lease(fb, cachelib_blobstore, "benchmark", "")
                        .expect("Memcache blobstore issues"),
                )
            }
        }
    });

    // Cut all the repetition around running a benchmark. Note that a fresh cachee shouldn't be needed,
    // as the warmup will fill it, and the key is randomised
    let mut run_benchmark =
        |bench: fn(&mut Criterion, CoreContext, Arc<dyn Blobstore>, &mut Runtime)| {
            bench(&mut criterion, ctx.clone(), blobstore.clone(), &mut runtime)
        };

    // Tests are run from here
    run_benchmark(single_puts::benchmark);
    run_benchmark(single_gets::benchmark);
    run_benchmark(parallel_same_blob_gets::benchmark);
    run_benchmark(parallel_different_blob_gets::benchmark);
    run_benchmark(parallel_puts::benchmark);

    runtime.shutdown_on_idle();
    criterion.final_summary();
}
