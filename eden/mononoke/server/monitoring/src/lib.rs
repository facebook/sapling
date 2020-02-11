/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding for service-level integration and monitoring.
//!

#![deny(warnings)]
#![feature(never_type)]

use std::thread::{self, JoinHandle};

use anyhow::{Error, Result};
use clap::ArgMatches;
use fbinit::FacebookInit;
use slog::{info, Logger};

use cmdlib::monitoring::ReadyFlagService;

// TODO: Stop using this one-off for Mononoke server, and instead use the one from cmdlib.
pub fn start_thrift_service<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    service: ReadyFlagService,
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
                    Box::new(service),
                )
                .expect("failure while running thrift service framework")
            })
            .map_err(Error::from)
    })
}
