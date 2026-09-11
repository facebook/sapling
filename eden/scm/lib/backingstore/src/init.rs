/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::sync::Arc;
#[cfg(feature = "scuba")]
use std::sync::LazyLock;
use std::sync::Once;
#[cfg(feature = "scuba")]
use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;

static RUST_INIT: Once = Once::new();

#[cfg(feature = "scuba")]
const EDENFS_EVENTS_QUEUE_SIZE: usize = 100;

/// Routes samples to EdenFS's process-wide events logger, while retaining the
/// legacy Scuba logger as a fallback until one is available. EdenServer owns
/// one XplatLogger and passes the same logger to every backing store, so the
/// first explicit binding intentionally remains installed for the process.
/// That binding holds only a weak C++ reference so the static does not prevent
/// XplatLogger's shutdown drain. Once it expires, late samples are discarded
/// rather than falling back to the legacy logger.
#[cfg(feature = "scuba")]
struct LateBindingSampleLogger {
    logger: OnceLock<Arc<dyn edenfs_telemetry::SampleLogger>>,
    fallback: OnceLock<Arc<dyn edenfs_telemetry::SampleLogger>>,
}

#[cfg(feature = "scuba")]
impl LateBindingSampleLogger {
    fn new() -> Self {
        Self {
            logger: OnceLock::new(),
            fallback: OnceLock::new(),
        }
    }

    fn set_fallback(&self, logger: Arc<dyn edenfs_telemetry::SampleLogger>) {
        let _ = self.fallback.set(logger);
    }

    fn set_logger(&self, logger: Arc<dyn edenfs_telemetry::SampleLogger>) {
        let _ = self.logger.set(logger);
    }
}

#[cfg(feature = "scuba")]
impl edenfs_telemetry::SampleLogger for LateBindingSampleLogger {
    fn log(&self, sample: edenfs_telemetry::EdenSample) -> anyhow::Result<()> {
        match self.logger.get().or_else(|| self.fallback.get()) {
            Some(logger) => logger.log(sample),
            None => Ok(()),
        }
    }
}

#[cfg(feature = "scuba")]
static EDENFS_EVENTS_LOGGER: LazyLock<Arc<LateBindingSampleLogger>> =
    LazyLock::new(|| Arc::new(LateBindingSampleLogger::new()));

macro_rules! maybe_add_edenfs_events_logger {
    ($subscriber:ident) => {
        #[cfg(feature = "scuba")]
        let logger: Arc<dyn edenfs_telemetry::SampleLogger> = (*EDENFS_EVENTS_LOGGER).clone();
        #[cfg(feature = "scuba")]
        let $subscriber = $subscriber.with(edenfs_telemetry::TracingLogger::new(logger));
    };
}

#[cfg(feature = "scuba")]
fn install_fallback_logger() {
    EDENFS_EVENTS_LOGGER.set_fallback(Arc::new(edenfs_telemetry::QueueingScubaLogger::new(
        edenfs_telemetry::new_scuba_logger(edenfs_telemetry::EDEN_EVENTS_SCUBA),
        EDENFS_EVENTS_QUEUE_SIZE,
    )));
}

/// We use this function to ensure everything we need to initialized as the Rust code may not be
/// called when EdenFS starts. Right now it only calls `env_logger::init` so we can see logs from
/// `edenapi` and other crates. In longer term we should bridge the logs to folly logging.
pub fn backingstore_global_init() {
    #[cfg(feature = "scuba")]
    install_fallback_logger();

    backingstore_global_init_impl();
}

pub(crate) fn backingstore_global_init_with_logger(
    logger: Arc<dyn edenfs_telemetry::SampleLogger>,
) {
    #[cfg(feature = "scuba")]
    EDENFS_EVENTS_LOGGER.set_logger(logger);

    #[cfg(not(feature = "scuba"))]
    drop(logger);

    backingstore_global_init_impl();
}

fn backingstore_global_init_impl() {
    RUST_INIT.call_once(|| {
        if let Some((var_name, _)) = identity::debug_env_var("LOG") {
            let env_filter = EnvFilter::from_env(var_name);
            let env_logger = FmtLayer::new()
                .with_span_events(FmtSpan::ACTIVE)
                .with_ansi(false)
                .with_writer(io::stderr);

            let subscriber = Registry::default().with(env_logger.with_filter(env_filter));

            maybe_add_edenfs_events_logger!(subscriber);

            if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!("Failed to set rust tracing subscriber: {e:?}");
            }
        } else {
            let subscriber = Registry::default();

            maybe_add_edenfs_events_logger!(subscriber);

            if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!("Failed to set rust tracing subscriber: {e:?}");
            }
        }

        env_logger::init();

        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);

        // Put progress into "no-op" mode to avoid overhead in eden.
        progress_model::Registry::main().disable(true);

        // For tests to trigger errors from instrumented code paths.
        testutil::failpoint::setup_global_fail_points();
    });
}

#[cfg(all(test, feature = "scuba"))]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use edenfs_telemetry::EdenSample;
    use edenfs_telemetry::SampleLogger;

    use super::*;

    struct CountingLogger {
        count: Arc<AtomicUsize>,
    }

    impl SampleLogger for CountingLogger {
        fn log(&self, _sample: EdenSample) -> anyhow::Result<()> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn late_binding_logger_accepts_logger_after_initial_use() {
        let fallback_count = Arc::new(AtomicUsize::new(0));
        let logger = LateBindingSampleLogger::new();
        logger.set_fallback(Arc::new(CountingLogger {
            count: Arc::clone(&fallback_count),
        }));

        logger.log(EdenSample::new()).unwrap();

        let installed_count = Arc::new(AtomicUsize::new(0));
        logger.set_logger(Arc::new(CountingLogger {
            count: Arc::clone(&installed_count),
        }));
        logger.log(EdenSample::new()).unwrap();

        assert_eq!(fallback_count.load(Ordering::Relaxed), 1);
        assert_eq!(installed_count.load(Ordering::Relaxed), 1);
    }
}
