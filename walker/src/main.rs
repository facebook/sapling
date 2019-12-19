/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use anyhow::Error;
use fbinit::FacebookInit;
use futures::IntoFuture;
use futures_ext::FutureExt;

use cmdlib::{args, helpers::create_runtime, monitoring};
use slog::{error, info};

mod blobstore;
mod count_objects;
#[macro_use]
mod graph;
mod progress;
mod scrub;
mod setup;
mod sizing;
mod state;
mod tail;
mod validate;
mod walk;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app_name = "walker";
    let matches = setup::setup_toplevel_app(app_name).get_matches();
    let logger = args::init_logging(fb, &matches);

    let future = match matches.subcommand() {
        (setup::COUNT_OBJECTS, Some(sub_m)) => {
            count_objects::count_objects(fb, logger.clone(), &matches, sub_m)
        }
        (setup::SCRUB_OBJECTS, Some(sub_m)) => {
            scrub::scrub_objects(fb, logger.clone(), &matches, sub_m)
        }
        (setup::COMPRESSION_BENEFIT, Some(sub_m)) => {
            sizing::compression_benefit(fb, logger.clone(), &matches, sub_m)
        }
        (setup::VALIDATE, Some(sub_m)) => validate::validate(fb, logger.clone(), &matches, sub_m),
        _ => Err(Error::msg("Invalid Arguments, pass --help for usage."))
            .into_future()
            .boxify(),
    };

    let mut runtime = create_runtime(None)?;

    monitoring::start_fb303_and_stats_agg(fb, &mut runtime, app_name, &logger, &matches)?;
    let res = runtime.block_on(future);

    runtime.shutdown_on_idle();

    info!(&logger, "Exiting...");
    res.map(|_| ()).map_err(|e| {
        error!(logger, "{:?}", e);
        e
    })
}
