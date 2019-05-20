// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::ArgMatches;
use failure_ext::Error;
use futures::future::ok;
use futures_ext::{BoxFuture, FutureExt};

use slog::{info, Logger};

pub fn subcommand_blacklist(
    logger: Logger,
    _matches: &ArgMatches<'_>,
    _sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    // TODO: implement the business logic
    info!(logger, "command not yet implemented");
    ok(()).boxify()
}
