/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::sync::Arc;
use std::sync::Once;

use parking_lot::Mutex;
use tracing_collector::TracingData;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;

static RUST_INIT: Once = Once::new();

/// We use this function to ensure everything we need to initialized as the Rust code may not be
/// called when EdenFS starts. Right now it only calls `env_logger::init` so we can see logs from
/// `edenapi` and other crates. In longer term we should bridge the logs to folly logging.
pub(crate) fn backingstore_global_init() {
    RUST_INIT.call_once(|| {
        if let Some((var_name, _)) = identity::debug_env_var("LOG") {
            let data = Arc::new(Mutex::new(TracingData::new()));
            let collector = tracing_collector::TracingCollector::new(data);
            let env_filter = EnvFilter::from_env(var_name);
            let env_logger = FmtLayer::new()
                .with_span_events(FmtSpan::ACTIVE)
                .with_ansi(false)
                .with_writer(io::stderr);
            let subscriber = Registry::default()
                .with(collector)
                .with(env_filter.and_then(env_logger));
            if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!("Failed to set rust tracing subscriber: {:?}", e);
            }
        }
        env_logger::init();

        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);
    });
}
