// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This benchmark generates linear stack with specified parameters, and then
//! measures how log it takes to convert it from Bonsai to Hg.

use benchmark_lib::{new_benchmark_repo, GenManifest};
use clap::{App, Arg};
use cmdlib::args;
use context::CoreContext;
use failure::Error;
use failure_ext::Result;
use futures::{future, Future};
use futures_ext::FutureExt;
use futures_stats::Timed;
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use tokio::runtime::Runtime;

fn run(ctx: CoreContext, rng_seed: u64) -> impl Future<Item = (), Error = Error> {
    println!("rng seed: {}", rng_seed);
    let mut rng = XorShiftRng::seed_from_u64(rng_seed); // reproducable Rng

    let delay_settings = Default::default();
    let repo = match new_benchmark_repo(delay_settings) {
        Err(err) => return future::err(err).left_future(),
        Ok(repo) => repo,
    };
    let mut gen = GenManifest::new();
    let settings = Default::default();
    gen.gen_stack(
        ctx.clone(),
        repo.clone(),
        &mut rng,
        &settings,
        None,
        std::iter::repeat(16).take(50),
    )
    .timed(move |stats, _| {
        println!("stack generated: {:?} {:?}", gen.size(), stats);
        Ok(())
    })
    .and_then(move |csid| {
        repo.get_hg_from_bonsai_changeset(ctx, csid)
            .timed(move |stats, result| {
                println!("bonsai->hg conversion: {:?}", stats);
                println!("{:?} -> {:?}", csid, result);
                Ok(())
            })
    })
    .map(|_| ())
    .right_future()
}

fn main() -> Result<()> {
    let app = {
        let app = App::new("mononoke benchmark").arg(
            Arg::with_name("seed")
                .short("s")
                .long("seed")
                .takes_value(true)
                .value_name("SEED")
                .help("seed changeset generator for u64 seed"),
        );
        let app = args::add_logger_args(app, true /* use glog */);
        args::add_cachelib_args(app, true /* hide_advanced_args */)
    };
    let matches = app.get_matches();
    args::init_cachelib(&matches);
    let logger = args::get_logger(&matches);
    let ctx = CoreContext::new_with_logger(logger.clone());
    let seed = matches
        .value_of("seed")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| rand::random());

    let mut runtime = Runtime::new()?;
    runtime.block_on(run(ctx, seed))
}
