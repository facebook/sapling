/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use anyhow::Error;
use anyhow::Result;
use clap::Args;
use fbinit::FacebookInit;
use services::Fb303Service;
use services::FbStatus;
use slog::info;
use slog::Logger;
use slog::Never;
use slog::SendSyncRefUnwindSafeDrain;

use crate::AppExtension;

/// Command line arguments that fb303 for service
#[derive(Args, Debug)]
pub struct Fb303Args {
    /// Port for fb303 service
    // thrift_port alias is for compatibility with mononoke server
    // TODO: switch mononoke server to use the same flags as everybody.
    #[clap(long, alias = "thrift_port", value_name = "PORT")]
    pub fb303_thrift_port: Option<i32>,
}

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

impl Fb303Args {
    /// This is a lower-level function that requires you to spawn the stats aggregation future
    /// yourself. This is useful if you'd like to be able to drop it in order to cancel it.
    ///
    /// Usually starting the fb303 server and stats aggregation is done by functions like
    /// `MononokeApp::run_with_monitoring_and_logging`.
    pub fn start_fb303_server<S: Fb303Service + Sync + Send + 'static>(
        &self,
        fb: FacebookInit,
        service_name: &str,
        logger: &Logger,
        service: S,
    ) -> Result<Option<()>, Error> {
        let service_name = service_name.to_string();
        self.fb303_thrift_port
            .map(|port| {
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
}

pub struct Fb303AppExtension;

impl AppExtension for Fb303AppExtension {
    type Args = Fb303Args;

    /// Hook executed after creating the log drain allowing for augmenting the logging.
    fn log_drain_hook(
        &self,
        args: &Fb303Args,
        drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>>,
    ) -> Result<Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>>> {
        if args.fb303_thrift_port.is_some() {
            Ok(Arc::new(slog_stats::StatsDrain::new(drain)))
        } else {
            Ok(drain)
        }
    }
}
