/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]
#![feature(async_closure)]
use anyhow::Error;
use fbinit::FacebookInit;
use futures::future::{self, FutureExt};

use cmdlib::{args::CachelibSettings, helpers::block_execute};

use walker_commands_impl::{corpus, scrub, setup, sizing, validate};

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    // FIXME: Investigate why some SQL queries kicked off by the walker take 30s or more.
    newfilenodes::disable_sql_timeouts();

    let app_name = "walker";
    let cachelib_defaults = CachelibSettings {
        cache_size: 2 * 1024 * 1024 * 1024,
        blobstore_cachelib_only: true,
        ..Default::default()
    };
    let matches = setup::setup_toplevel_app(app_name, cachelib_defaults).get_matches(fb)?;
    let logger = matches.logger();

    let sub_matches = &matches.subcommand();
    let future = match sub_matches {
        (setup::COMPRESSION_BENEFIT, Some(sub_m)) => {
            sizing::compression_benefit(fb, logger.clone(), &matches, sub_m).boxed()
        }
        (setup::CORPUS, Some(sub_m)) => corpus::corpus(fb, logger.clone(), &matches, sub_m).boxed(),
        (setup::SCRUB, Some(sub_m)) => {
            scrub::scrub_objects(fb, logger.clone(), &matches, sub_m).boxed()
        }
        (setup::VALIDATE, Some(sub_m)) => {
            validate::validate(fb, logger.clone(), &matches, sub_m).boxed()
        }
        _ => {
            future::err::<_, Error>(Error::msg("Invalid Arguments, pass --help for usage.")).boxed()
        }
    };

    block_execute(
        future,
        fb,
        app_name,
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
