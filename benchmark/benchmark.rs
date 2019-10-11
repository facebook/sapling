/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! This benchmark generates linear stack with specified parameters, and then
//! measures how log it takes to convert it from Bonsai to Hg.

#![deny(warnings)]

use benchmark_lib::{new_benchmark_repo, GenManifest};
use blobrepo::BlobRepo;
use clap::{App, Arg};
use cmdlib::args;
use context::CoreContext;
use derived_data::BonsaiDerived;
use failure::{err_msg, format_err, Error};
use failure_ext::Result;
use fbinit::FacebookInit;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::Timed;
use mononoke_types::ChangesetId;
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use std::sync::Arc;
use tokio::runtime::Runtime;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

const HG_CHANGESET_TYPE: &'static str = "hg-changeset";

type DeriveFn = Arc<dyn Fn(ChangesetId) -> BoxFuture<String, Error> + Send + Sync + 'static>;

fn run(
    ctx: CoreContext,
    repo: BlobRepo,
    rng_seed: u64,
    derive: DeriveFn,
) -> impl Future<Item = (), Error = Error> {
    println!("rng seed: {}", rng_seed);
    let mut rng = XorShiftRng::seed_from_u64(rng_seed); // reproducable Rng

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
        derive(csid).timed(move |stats, result| {
            println!("bonsai conversion: {:?}", stats);
            println!("{:?} -> {:?}", csid, result);
            Ok(())
        })
    })
    .map(|_| ())
}

fn derive_fn(ctx: CoreContext, repo: BlobRepo, derive_type: Option<&str>) -> Result<DeriveFn> {
    match derive_type {
        None => Err(err_msg("required `type` argument is missing")),
        Some(HG_CHANGESET_TYPE) => {
            let derive_hg_changeset = move |csid| {
                repo.get_hg_from_bonsai_changeset(ctx.clone(), csid)
                    .map(|hg_csid| hg_csid.to_string())
                    .boxify()
            };
            Ok(Arc::new(derive_hg_changeset))
        }
        Some(RootUnodeManifestId::NAME) => {
            let mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
            let derive_unodes = move |csid| {
                RootUnodeManifestId::derive(ctx.clone(), repo.clone(), mapping.clone(), csid)
                    .map(|root| root.manifest_unode_id().to_string())
                    .boxify()
            };
            Ok(Arc::new(derive_unodes))
        }
        Some(derived_type) => Err(format_err!("unknown derived data type: {}", derived_type)),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = {
        let app = App::new("mononoke benchmark")
            .arg(
                Arg::with_name("seed")
                    .short("s")
                    .long("seed")
                    .takes_value(true)
                    .value_name("SEED")
                    .help("seed changeset generator for u64 seed"),
            )
            .arg(
                Arg::with_name("type")
                    .required(true)
                    .index(1)
                    .possible_values(&[HG_CHANGESET_TYPE, RootUnodeManifestId::NAME])
                    .help("derived data type"),
            );
        let app = args::add_logger_args(app);
        args::add_cachelib_args(app, true /* hide_advanced_args */)
    };
    let matches = app.get_matches();

    args::init_cachelib(fb, &matches);
    let logger = args::init_logging(&matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = new_benchmark_repo(fb, Default::default())?;

    let seed = matches
        .value_of("seed")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| rand::random());
    let derive = derive_fn(ctx.clone(), repo.clone(), matches.value_of("type"))?;

    let mut runtime = Runtime::new()?;
    runtime.block_on(run(ctx, repo, seed, derive))
}
