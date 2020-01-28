/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Scaffolding for service-level integration and monitoring.
//!

#![deny(warnings)]
#![feature(never_type)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

use anyhow::{Error, Result};
use clap::ArgMatches;
use fbinit::FacebookInit;
use services::{self, Fb303Service, FbStatus};
use slog::{info, Logger};

#[derive(Clone)]
pub struct MononokeService {
    ready: Arc<AtomicBool>,
}

impl MononokeService {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::Relaxed);
    }
}

impl Fb303Service for MononokeService {
    fn getStatus(&self) -> FbStatus {
        if self.ready.load(Ordering::Relaxed) {
            FbStatus::Alive
        } else {
            FbStatus::Starting
        }
    }
}

pub fn start_thrift_service<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    service: MononokeService,
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
