/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::time::Duration;

use clap::Arg;
use criterion::Criterion;

use blobstore_factory::make_blobstore;
use cmdlib::args;
use context::CoreContext;

mod parallel_puts;
mod single_puts;

pub const KB: usize = 1024;
pub const MB: usize = KB * 1024;
const ARG_STORAGE_CONFIG_NAME: &'static str = "storage-config-name";
const ARG_SAVE_BASELINE: &'static str = "save-baseline";
const ARG_USE_BASELINE: &'static str = "use-baseline";

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

    let logger = args::init_logging(fb, &matches);
    args::init_cachelib(fb, &matches, None);

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

    let blobstore = runtime
        .block_on(make_blobstore(
            fb,
            storage_config.blobstore,
            mysql_options,
            blobstore_factory::ReadOnlyStorage(false),
            blobstore_options,
            logger,
        ))
        .expect("Could not make blobstore");

    // Tests are run from here
    single_puts::benchmark(&mut criterion, ctx.clone(), blobstore.clone(), &mut runtime);
    parallel_puts::benchmark(&mut criterion, ctx.clone(), blobstore.clone(), &mut runtime);

    runtime.shutdown_on_idle();
    criterion.final_summary();
}
