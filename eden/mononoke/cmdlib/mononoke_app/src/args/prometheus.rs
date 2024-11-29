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
use slog::info;
use slog::Logger;
use slog::Never;
use slog::SendSyncRefUnwindSafeDrain;

use crate::AppExtension;

/// Command line arguments for prometheus metrics server
#[derive(Args, Debug)]
pub struct PrometheusArgs {
    // TODO: implementation
}

impl PrometheusArgs {
    /// This is a lower-level function that requires you to spawn the stats aggregation future
    /// yourself. This is useful if you'd like to be able to drop it in order to cancel it.
    ///
    /// Usually starting the fb303 server and stats aggregation is done by functions like
    /// `MononokeApp::run_with_monitoring_and_logging`.
    pub fn start_monitoring_server<S>(
        &self,
        _fb: FacebookInit,
        _service_name: &str,
        _logger: &Logger,
        _service: S,
    ) -> Result<Option<()>, Error> {
        // TODO: implementation
    }
}

pub struct PrometheusAppExtension;

impl AppExtension for PrometheusAppExtension {
    type Args = PrometheusArgs;

    /// Hook executed after creating the log drain allowing for augmenting the logging.
    fn log_drain_hook(
        &self,
        args: &MonitoringArgs,
        drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>>,
    ) -> Result<Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>>> {
        if args.fb303_thrift_port.is_some() {
            Ok(Arc::new(slog_stats::StatsDrain::new(drain)))
        } else {
            Ok(drain)
        }
    }
}
