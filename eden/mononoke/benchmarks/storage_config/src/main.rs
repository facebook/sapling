/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use blobstore::Blobstore;
use blobstore_factory::make_blobstore;
use cacheblob::CachelibBlobstoreOptions;
use cacheblob::new_memcache_blobstore;
use clap::Parser;
use context::CoreContext;
use criterion::Criterion;
use environment::Caching;
use mononoke_app::MononokeAppBuilder;
use tokio::runtime::Handle;

mod parallel_puts;
mod single_puts;

mod parallel_different_blob_gets;
mod parallel_same_blob_gets;
mod single_gets;

pub const KB: usize = 1024;
pub const MB: usize = KB * 1024;

/// Benchmark storage config.
#[derive(Parser)]
struct BenchmarkArgs {
    /// The name of the storage config to benchmark.
    #[clap(long)]
    storage_config_name: String,

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

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb).build::<BenchmarkArgs>()?;
    let args: BenchmarkArgs = app.args()?;

    let mut criterion = Criterion::default()
        .measurement_time(Duration::from_secs(60))
        .sample_size(10)
        .warm_up_time(Duration::from_secs(60));

    if let Some(baseline) = &args.save_baseline {
        criterion = criterion.save_baseline(baseline.to_string());
    }
    if let Some(baseline) = &args.use_baseline {
        criterion = criterion.retain_baseline(baseline.to_string(), true);
    }

    for filter in args.filter.iter() {
        criterion = criterion.with_filter(filter.to_string())
    }

    let caching = app.environment().caching;
    let logger = app.logger();
    let config_store = app.config_store();

    let storage_configs = app.storage_configs();
    let storage_config = storage_configs
        .storage
        .get(&args.storage_config_name)
        .ok_or_else(|| anyhow!("unknown storage config"))?;
    let mysql_options = app.mysql_options();
    let blobstore_options = app.blobstore_options();
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
            Caching::LocalOnly(local_cache_config) => repo_factory::cachelib_blobstore(
                blobstore,
                local_cache_config.blobstore_cache_shards,
                &CachelibBlobstoreOptions::default(),
            )
            .context("repo_factory::cachelib_blobstore failed")?,
            Caching::Enabled(local_cache_config) => {
                let cachelib_blobstore = repo_factory::cachelib_blobstore(
                    blobstore,
                    local_cache_config.blobstore_cache_shards,
                    &CachelibBlobstoreOptions::default(),
                )
                .context("repo_factory::cachelib_blobstore failed")?;
                Arc::new(
                    new_memcache_blobstore(fb, cachelib_blobstore, "benchmark", "")
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
        let runtime = app.runtime();
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
