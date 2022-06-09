/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::sync::Arc;
use std::sync::Once;

use parking_lot::Mutex;
use tracing::Level;
use tracing_collector::TracingData;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

static RUST_INIT: Once = Once::new();

/// We use this function to ensure everything we need to initialized as the Rust code may not be
/// called when EdenFS starts. Right now it only calls `env_logger::init` so we can see logs from
/// `edenapi` and other crates. In longer term we should bridge the logs to folly logging.
pub(crate) fn backingstore_global_init() {
    RUST_INIT.call_once(|| {
        if env::var("EDENSCM_LOG").is_ok() {
            let data = Arc::new(Mutex::new(TracingData::new()));
            let collector = tracing_collector::default_collector(data, Level::TRACE);
            let env_filter = EnvFilter::from_env("EDENSCM_LOG");
            let env_logger = FmtLayer::new()
                .with_span_events(FmtSpan::ACTIVE)
                .with_ansi(false);
            let collector = collector.with(env_filter.and_then(env_logger));
            if let Err(e) = tracing::subscriber::set_global_default(collector) {
                eprintln!("Failed to set rust tracing subscriber: {:?}", e);
            }
        }
        env_logger::init();

        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);
    });
}
