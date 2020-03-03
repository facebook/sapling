/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding for service-level integration and monitoring.

use std::thread::{self};

use anyhow::{format_err, Error};
use clap::ArgMatches;
use fbinit::FacebookInit;
use futures_old::Future;
use services::{self, Fb303Service, FbStatus};
use slog::{info, Logger};
use stats::schedule_stats_aggregation;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

// Re-eport AliveService for convenience so callers do not have to get the services dependency to
// get AliveService.
pub use services::AliveService;

/// A FB303 service that reports healthy once set_ready has been called.
#[derive(Clone)]
pub struct ReadyFlagService {
    ready: Arc<AtomicBool>,
}

impl ReadyFlagService {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::Relaxed);
    }
}

impl Fb303Service for ReadyFlagService {
    fn getStatus(&self) -> FbStatus {
        if self.ready.load(Ordering::Relaxed) {
            FbStatus::Alive
        } else {
            FbStatus::Starting
        }
    }
}

/// `service_name` should match tupperware to avoid confusion.
/// e.g. for mononoke/blobstore_healer, pass blobstore_healer
pub fn start_fb303_and_stats_agg<S: Fb303Service + Sync + Send + 'static>(
    fb: FacebookInit,
    runtime: &mut tokio_compat::runtime::Runtime,
    service_name: &str,
    logger: &Logger,
    matches: &ArgMatches,
    service: S,
) -> Result<(), Error> {
    let service_name = service_name.to_string();
    if let Some(()) = start_fb303_server(fb, &service_name, logger, matches, service)? {
        let scheduler = schedule_stats_aggregation()
            .map_err(|e| format_err!("Failed to start stats aggregation {:?}", e))?;

        runtime.spawn(scheduler.map_err(|e| {
            eprintln!("Unexpected error from stats aggregation: {:#?}", e);
        }));
    }
    Ok(())
}

/// This is a lower-level function that requires you to spawn the stats aggregation future
/// yourself. This is useful if you'd like to be able to drop it in order to cancel it.
pub fn start_fb303_server<S: Fb303Service + Sync + Send + 'static>(
    fb: FacebookInit,
    service_name: &str,
    logger: &Logger,
    matches: &ArgMatches,
    service: S,
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
                        Box::new(service),
                    )
                    .expect("failure while running thrift service framework")
                })
                .map_err(Error::from)?;

            Ok(())
        })
        .transpose()
}
