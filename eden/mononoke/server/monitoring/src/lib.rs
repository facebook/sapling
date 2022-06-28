/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding for service-level integration and monitoring.
//!

#![feature(never_type)]

use std::thread;
use std::thread::JoinHandle;

use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use slog::info;
use slog::Logger;

use cmdlib::args::MononokeMatches;
use cmdlib::monitoring::ReadyFlagService;

// TODO: Stop using this one-off for Mononoke server, and instead use the one from cmdlib.
pub fn start_thrift_service<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'a>,
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
