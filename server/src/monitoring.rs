// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding for service-level integration and monitoring.

use std::thread::{self, JoinHandle};

use clap::ArgMatches;
use slog::Logger;
use tokio_core::reactor::Core;

use services::{self, Fb303Service, FbStatus};
use stats;

use errors::*;

pub(crate) fn start_stats() -> Result<JoinHandle<!>> {
    Ok(thread::Builder::new()
        .name("stats_aggregation".to_owned())
        .spawn(move || {
            let mut core = Core::new().expect("failed to create tokio core");
            let scheduler = stats::schedule_stats_aggregation(&core.handle())
                .expect("failed to create stats aggregation scheduler");
            core.run(scheduler).expect("stats scheduler failed");
            // stats scheduler shouldn't finish successfully
            unreachable!()
        })?)
}

struct MononokeService;

impl Fb303Service for MononokeService {
    fn getStatus(&self) -> FbStatus {
        // TODO: return Starting while precaching is active.
        FbStatus::Alive
    }
}

pub(crate) fn start_thrift_service<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> Option<Result<JoinHandle<!>>> {
    matches.value_of("thrift_port").map(|port| {
        let port = port.parse().expect("Failed to parse thrift_port as number");
        info!(logger, "Initializing thrift server on port {}", port);

        thread::Builder::new()
            .name("thrift_service".to_owned())
            .spawn(move || {
                services::run_service_framework(
                    "mononoke_server",
                    port,
                    0, // Disables separate status http server
                    Box::new(MononokeService),
                ).expect("failure while running thrift service framework")
            })
            .map_err(Error::from)
    })
}
