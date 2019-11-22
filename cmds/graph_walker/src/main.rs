/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use failure_ext::{err_msg, Error};
use fbinit::FacebookInit;
use futures::IntoFuture;
use futures_ext::FutureExt;

use cmdlib::{args, monitoring};
use slog::{error, info};

mod blobstore;
mod count_objects;
mod graph;
mod parse_args;
mod progress;
mod scrub;
mod state;
mod walk;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app_name = "graph_walker";
    let matches = parse_args::setup_toplevel_app(app_name).get_matches();
    let logger = args::init_logging(fb, &matches);

    let future = match matches.subcommand() {
        (parse_args::COUNT_OBJECTS, Some(sub_m)) => {
            count_objects::count_objects(fb, logger.clone(), &matches, sub_m)
        }
        (parse_args::SCRUB_OBJECTS, Some(sub_m)) => {
            scrub::scrub_objects(fb, logger.clone(), &matches, sub_m)
        }
        _ => Err(err_msg("Invalid Arguments, pass --help for usage."))
            .into_future()
            .boxify(),
    };

    let mut runtime = tokio::runtime::Runtime::new()?;

    monitoring::start_fb303_and_stats_agg(fb, &mut runtime, app_name, &logger, &matches)?;
    let res = runtime.block_on(future);

    info!(&logger, "Waiting for in-flight requests to finish...");
    runtime.shutdown_on_idle();

    info!(&logger, "Exiting...");
    res.map(|_| ()).map_err(|e| {
        error!(logger, "{:?}", e);
        e
    })
}
