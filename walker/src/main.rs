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
use futures_preview::compat::Future01CompatExt;

use cmdlib::{args, helpers::block_execute};

mod blobstore;
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
        (setup::SCRUB, Some(sub_m)) => scrub::scrub_objects(fb, logger.clone(), &matches, sub_m),
        (setup::COMPRESSION_BENEFIT, Some(sub_m)) => {
            sizing::compression_benefit(fb, logger.clone(), &matches, sub_m)
        }
        (setup::VALIDATE, Some(sub_m)) => validate::validate(fb, logger.clone(), &matches, sub_m),
        _ => Err(Error::msg("Invalid Arguments, pass --help for usage."))
            .into_future()
            .boxify(),
    };

    block_execute(future.compat(), fb, app_name, &logger, &matches)
}
