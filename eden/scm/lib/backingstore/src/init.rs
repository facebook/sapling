/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::sync::Arc;
use std::sync::Once;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;

static RUST_INIT: Once = Once::new();

macro_rules! maybe_add_scuba_logger {
    ($var:ident) => {
        // Tracing subscriber that logs to edenfs_events.
        #[cfg(feature = "scuba")]
        let $var = $var.with(edenfs_telemetry::TracingLogger::new(Arc::new(
            edenfs_telemetry::QueueingScubaLogger::new(
                edenfs_telemetry::new_scuba_logger(edenfs_telemetry::EDEN_EVENTS_SCUBA),
                100,
            ),
        )));
    };
}

/// We use this function to ensure everything we need to initialized as the Rust code may not be
/// called when EdenFS starts. Right now it only calls `env_logger::init` so we can see logs from
/// `edenapi` and other crates. In longer term we should bridge the logs to folly logging.
pub fn backingstore_global_init() {
    RUST_INIT.call_once(|| {
        if let Some((var_name, _)) = identity::debug_env_var("LOG") {
            let env_filter = EnvFilter::from_env(var_name);
            let env_logger = FmtLayer::new()
                .with_span_events(FmtSpan::ACTIVE)
                .with_ansi(false)
                .with_writer(io::stderr);

            let subscriber = Registry::default().with(env_logger.with_filter(env_filter));

            maybe_add_scuba_logger!(subscriber);

            if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!("Failed to set rust tracing subscriber: {:?}", e);
            }
        } else {
            let subscriber = Registry::default();

            maybe_add_scuba_logger!(subscriber);

            if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!("Failed to set rust tracing subscriber: {:?}", e);
            }
        }

        env_logger::init();

        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);

        #[cfg(feature = "cas")]
        rich_cas_client::init();

        // Put progress into "no-op" mode to avoid overhead in eden.
        progress_model::Registry::main().disable(true);

        // For tests to trigger errors from instrumented code paths.
        testutil::failpoint::setup_global_fail_points();
    });
}
