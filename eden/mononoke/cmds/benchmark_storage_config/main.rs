/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use clap_old::Arg;
use criterion::Criterion;
use tokio::runtime::Handle;

use blobstore::Blobstore;
use blobstore_factory::make_blobstore;
use cacheblob::new_memcache_blobstore_no_lease;
use cacheblob::CachelibBlobstoreOptions;
use cmdlib::args;
use context::CoreContext;
use environment::Caching;

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

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("benchmark_storage_config")
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
    let matches = app.get_matches(fb)?;

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

    let caching = matches.caching();
    let logger = matches.logger();
    let runtime = matches.runtime();

    let config_store = matches.config_store();

    let config_name = matches
        .value_of(ARG_STORAGE_CONFIG_NAME)
        .context("No storage config name")?;

    let storage_config = args::load_storage_configs(config_store, &matches)
        .context("Could not read storage configs")?
        .storage
        .remove(config_name)
        .ok_or_else(|| anyhow!("unknown storage config"))?;
    let mysql_options = matches.mysql_options();
    let blobstore_options = matches.blobstore_options();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let blobstore = || async {
        let blobstore = make_blobstore(
            fb,
            storage_config.blobstore.clone(),
            mysql_options,
            blobstore_factory::ReadOnlyStorage(false),
            blobstore_options,
            logger,
            config_store,
            &blobstore_factory::default_scrub_handler(),
            None,
        )
        .await
        .context("Could not make blobstore")?;
        let blobstore = match caching {
            Caching::Disabled => blobstore,
            Caching::CachelibOnlyBlobstore(cache_shards) => repo_factory::cachelib_blobstore(
                blobstore,
                cache_shards,
                &CachelibBlobstoreOptions::default(),
            )
            .context("repo_factory::cachelib_blobstore failed")?,
            Caching::Enabled(cache_shards) => {
                let cachelib_blobstore = repo_factory::cachelib_blobstore(
                    blobstore,
                    cache_shards,
                    &CachelibBlobstoreOptions::default(),
                )
                .context("repo_factory::cachelib_blobstore failed")?;
                Arc::new(
                    new_memcache_blobstore_no_lease(fb, cachelib_blobstore, "benchmark", "")
                        .context("Memcache blobstore issues")?,
                )
            }
        };
        Ok::<_, Error>(blobstore)
    };

    // Cut all the repetition around running a benchmark. Note that a fresh cachee shouldn't be needed,
    // as the warmup will fill it, and the key is randomised
    let mut run_benchmark = {
        let criterion = &mut criterion;
        move |bench: fn(&mut Criterion, CoreContext, Arc<dyn Blobstore>, &Handle)| -> Result<(), Error> {
            let blobstore = runtime.block_on(blobstore())?;
            bench(criterion, ctx.clone(), blobstore, runtime);
            Ok(())
        }
    };

    // Tests are run from here
    run_benchmark(single_puts::benchmark)?;
    run_benchmark(single_gets::benchmark)?;
    run_benchmark(parallel_same_blob_gets::benchmark)?;
    run_benchmark(parallel_different_blob_gets::benchmark)?;
    run_benchmark(parallel_puts::benchmark)?;

    criterion.final_summary();
    Ok(())
}
