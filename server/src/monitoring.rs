/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Scaffolding for service-level integration and monitoring.

use std::thread::{self, JoinHandle};

use clap::ArgMatches;
use fbinit::FacebookInit;
use services::{self, Fb303Service, FbStatus};
use slog::{info, Logger};

use ready_state::ReadyState;

use crate::errors::*;

struct MononokeService {
    ready: ReadyState,
}

impl Fb303Service for MononokeService {
    fn getStatus(&self) -> FbStatus {
        // TODO: return Starting while precaching is active.
        if self.ready.is_ready() {
            FbStatus::Alive
        } else {
            FbStatus::Starting
        }
    }
}

pub(crate) fn start_thrift_service<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    ready: ReadyState,
) -> Option<Result<JoinHandle<!>>> {
    matches.value_of("thrift_port").map(|port| {
        let port = port.parse().expect("Failed to parse thrift_port as number");
        info!(logger, "Initializing thrift server on port {}", port);

        thread::Builder::new()
            .name("thrift_service".to_owned())
            .spawn(move || {
                services::run_service_framework(
                    fb,
                    "mononoke_server",
                    port,
                    0, // Disables separate status http server
                    Box::new(MononokeService { ready }),
                )
                .expect("failure while running thrift service framework")
            })
            .map_err(Error::from)
    })
}
