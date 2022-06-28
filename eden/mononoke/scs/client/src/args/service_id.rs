/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for Service Identities

use clap::App;
use clap::Arg;
use clap::ArgMatches;

pub(crate) const ARG_SERVICE_ID: &str = "SERVICE_ID";

/// Add arguments to specify a service identity.
pub(crate) fn add_service_id_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_SERVICE_ID)
            .long("service-id")
            .takes_value(true)
            .number_of_values(1)
            .help("Service identity to perform write operation as"),
    )
}

/// Get the service identity specified.
pub(crate) fn get_service_id<'a>(matches: &'a ArgMatches) -> Option<&'a str> {
    matches.value_of(ARG_SERVICE_ID)
}
