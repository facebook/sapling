/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This benchmark generates linear stack with specified parameters, and then
//! measures how log it takes to convert it from Bonsai to Hg.

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use clap::Arg;
use cmdlib::args;
use cmdlib::args::get_and_parse_opt;
use cmdlib::args::ArgType;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use derived_data_utils::derived_data_utils;
use derived_data_utils::POSSIBLE_DERIVED_TYPES;
use fbinit::FacebookInit;
use futures::future::TryFutureExt;
use futures_stats::futures03::TimedFutureExt;
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use simulated_repo::new_benchmark_repo;
use simulated_repo::DelaySettings;
use simulated_repo::GenManifest;
use tokio::runtime::Runtime;

const ARG_SEED: &str = "seed";
const ARG_TYPE: &str = "type";
const ARG_STACK_SIZE: &str = "stack-size";
const ARG_BLOBSTORE_PUT_MEAN_SECS: &str = "blobstore-put-mean-secs";
const ARG_BLOBSTORE_PUT_STDDEV_SECS: &str = "blobstore-put-stddev-secs";
const ARG_BLOBSTORE_GET_MEAN_SECS: &str = "blobstore-get-mean-secs";
const ARG_BLOBSTORE_GET_STDDEV_SECS: &str = "blobstore-get-stddev-secs";
const ARG_USE_BACKFILL_MODE: &str = "use-backfill-mode";

pub type Normal = rand_distr::Normal<f64>;

async fn run(
    ctx: CoreContext,
    repo: BlobRepo,
    rng_seed: u64,
    stack_size: usize,
    derived_data_type: &str,
    use_backfill_mode: bool,
) -> Result<(), Error> {
    println!("rng seed: {}", rng_seed);
    let mut rng = XorShiftRng::seed_from_u64(rng_seed); // reproducable Rng

    let mut gen = GenManifest::new();
    let settings = Default::default();
    let (stats, csidq) = gen
        .gen_stack(
            ctx.clone(),
            repo.clone(),
            &mut rng,
            &settings,
            None,
            std::iter::repeat(16).take(stack_size),
        )
        .timed()
        .await;
    println!("stack generated: {:?} {:?}", gen.size(), stats);

    let csid = csidq?;
    let derive_utils = derived_data_utils(ctx.fb, &repo, derived_data_type)?;

    let (stats2, result) = if use_backfill_mode {
        derive_utils
            .backfill_batch_dangerous(
                ctx,
                repo.clone(),
                vec![csid],
                true, /* parallel */
                None, /* No gaps */
            )
            .map_ok(|_| ())
            .timed()
            .await
    } else {
        derive_utils
            .derive(ctx.clone(), repo.clone(), csid)
            .map_ok(|_| ())
            .timed()
            .await
    };

    println!("bonsai conversion: {:?}", stats2);
    println!("{:?} -> {:?}", csid.to_string(), result);
    Ok(())
}

fn parse_norm_distribution(
    matches: &MononokeMatches,
    mean_key: &str,
    stddev_key: &str,
) -> Result<Option<Normal>, Error> {
    let put_mean = get_and_parse_opt(matches, mean_key);
    let put_stddev = get_and_parse_opt(matches, stddev_key);
    match (put_mean, put_stddev) {
        (Some(put_mean), Some(put_stddev)) => {
            let dist = Normal::new(put_mean, put_stddev)
                .map_err(|err| anyhow!("can't create normal distribution {:?}", err))?;
            Ok(Some(dist))
        }
        _ => Ok(None),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = args::MononokeAppBuilder::new("mononoke benchmark")
        .without_arg_types(vec![
            ArgType::Config,
            ArgType::Repo,
            ArgType::Tunables,
            ArgType::Runtime, // we construct our own runtime, so these args would do nothing
        ])
        .with_advanced_args_hidden()
        .build()
        .arg(
            Arg::with_name(ARG_SEED)
                .short("s")
                .long(ARG_SEED)
                .takes_value(true)
                .value_name(ARG_SEED)
                .help("seed changeset generator for u64 seed"),
        )
        .arg(
            Arg::with_name(ARG_STACK_SIZE)
                .long(ARG_STACK_SIZE)
                .takes_value(true)
                .value_name(ARG_STACK_SIZE)
                .help("Size of the generated stack"),
        )
        .arg(
            Arg::with_name(ARG_BLOBSTORE_PUT_MEAN_SECS)
                .long(ARG_BLOBSTORE_PUT_MEAN_SECS)
                .takes_value(true)
                .value_name(ARG_BLOBSTORE_PUT_MEAN_SECS)
                .help("Mean value of blobstore put calls. Will be used to emulate blobstore delay"),
        )
        .arg(
            Arg::with_name(ARG_BLOBSTORE_PUT_STDDEV_SECS)
                .long(ARG_BLOBSTORE_PUT_STDDEV_SECS)
                .takes_value(true)
                .value_name(ARG_BLOBSTORE_PUT_STDDEV_SECS)
                .help(
                    "Std deviation of blobstore put calls. Will be used to emulate blobstore delay",
                ),
        )
        .arg(
            Arg::with_name(ARG_BLOBSTORE_GET_MEAN_SECS)
                .long(ARG_BLOBSTORE_GET_MEAN_SECS)
                .takes_value(true)
                .value_name(ARG_BLOBSTORE_GET_MEAN_SECS)
                .help("Mean value of blobstore put calls. Will be used to emulate blobstore delay"),
        )
        .arg(
            Arg::with_name(ARG_BLOBSTORE_GET_STDDEV_SECS)
                .long(ARG_BLOBSTORE_GET_STDDEV_SECS)
                .takes_value(true)
                .value_name(ARG_BLOBSTORE_GET_STDDEV_SECS)
                .help(
                    "Std deviation of blobstore put calls. Will be used to emulate blobstore delay",
                ),
        )
        .arg(
            Arg::with_name(ARG_TYPE)
                .required(true)
                .index(1)
                .possible_values(POSSIBLE_DERIVED_TYPES)
                .help("derived data type"),
        )
        .arg(
            Arg::with_name(ARG_USE_BACKFILL_MODE)
                .long(ARG_USE_BACKFILL_MODE)
                .takes_value(false)
                .help("Use backfilling mode while deriving"),
        )
        .get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let mut delay: DelaySettings = Default::default();
    if let Some(dist) = parse_norm_distribution(
        &matches,
        ARG_BLOBSTORE_PUT_MEAN_SECS,
        ARG_BLOBSTORE_PUT_STDDEV_SECS,
    )? {
        delay.blobstore_put_dist = dist;
    }
    if let Some(dist) = parse_norm_distribution(
        &matches,
        ARG_BLOBSTORE_GET_MEAN_SECS,
        ARG_BLOBSTORE_GET_STDDEV_SECS,
    )? {
        delay.blobstore_get_dist = dist;
    }

    let repo = new_benchmark_repo(fb, delay)?;

    let seed = matches
        .value_of(ARG_SEED)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(rand::random);

    let stack_size: usize = matches
        .value_of(ARG_STACK_SIZE)
        .unwrap_or("50")
        .parse()
        .expect("stack size must be a positive integer");

    let runtime = Runtime::new()?;
    let derived_data_type = matches
        .value_of(ARG_TYPE)
        .ok_or_else(|| anyhow!("{} not set", ARG_TYPE))?;
    runtime.block_on(run(
        ctx,
        repo,
        seed,
        stack_size,
        derived_data_type,
        matches.is_present(ARG_USE_BACKFILL_MODE),
    ))
}
