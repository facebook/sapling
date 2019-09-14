// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding for service-level integration and monitoring.

use std::thread::{self};

use clap::ArgMatches;
use failure_ext::{format_err, Error};
use fbinit::FacebookInit;
use futures::future::Future;
use services::{self, AliveService};
use slog::{info, Logger};
use stats::schedule_stats_aggregation;

/// `service_name` should match tupperware to avoid confusion.
/// e.g. for mononoke/blobstore_healer, pass blobstore_healer
pub fn start_fb303_and_stats_agg(
    fb: FacebookInit,
    runtime: &mut tokio::runtime::Runtime,
    service_name: &str,
    logger: &Logger,
    matches: &ArgMatches,
) -> Result<Option<()>, Error> {
    let service_name = service_name.to_string();
    matches
        .value_of("fb303-thrift-port")
        .map(|port| {
            let port = port.parse().map_err(Error::from)?;
            info!(logger, "Initializing fb303 thrift server on port {}", port);

            thread::Builder::new()
                .name("fb303_thrift_service".to_owned())
                .spawn(move || {
                    services::run_service_framework(
                        fb,
                        service_name,
                        port,
                        0, // Disables separate status http server
                        Box::new(AliveService),
                    )
                    .expect("failure while running thrift service framework")
                })
                .map_err(Error::from)
                .and_then(|_| {
                    schedule_stats_aggregation()
                        .map_err(|e| format_err!("Failed to start stats aggregation {:?}", e))
                })
                .map(|stats_aggregation| {
                    runtime.spawn(stats_aggregation.map_err(|e| {
                        eprintln!("Unexpected error from stats aggregation: {:#?}", e);
                    }));
                    ()
                })
        })
        .transpose()
}
