/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use slog::info;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Import Repository")
        .with_advanced_args_hidden()
        .build()
        .version("0.0.0")
        .about("Automating repository imports");

    let matches = app.get_matches();
    args::init_cachelib(fb, &matches, None);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger);

    info!(ctx.logger(), "Hello world!");
    Ok(())
}
