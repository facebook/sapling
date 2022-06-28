/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding for service-level integration and monitoring.

use std::thread;

use anyhow::Error;
use fbinit::FacebookInit;
pub use mononoke_app::fb303::ReadyFlagService;
use services;
use services::Fb303Service;
use slog::info;
use slog::Logger;

use crate::args::MononokeMatches;

// Re-eport AliveService for convenience so callers do not have to get the services dependency to
// get AliveService.
pub use services::AliveService;

/// This is a lower-level function that requires you to spawn the stats aggregation future
/// yourself. This is useful if you'd like to be able to drop it in order to cancel it.
pub fn start_fb303_server<S: Fb303Service + Sync + Send + 'static>(
    fb: FacebookInit,
    service_name: &str,
    logger: &Logger,
    matches: &MononokeMatches,
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
