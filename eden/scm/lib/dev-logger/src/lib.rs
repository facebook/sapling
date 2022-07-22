/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Convenient env_logger for testing purpose.
//!
//! # Example
//!
//! ```
//! // In lib.rs:
//! #[cfg(test)]
//! dev_logger::init!();
//!
//! // In test function:
//! tracing::info!(name = "message");
//!
//! // Set RUST_LOG=info and run the test.
//! ```

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

pub use ctor::ctor;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::Subscriber;
use tracing_subscriber::EnvFilter;

/// Initialize tracing and env_logger for adhoc logging (ex. in a library test)
/// purpose.
pub fn init() {
    let builder = Subscriber::builder()
        .with_env_filter(EnvFilter::from_env("LOG"))
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .with_span_events(FmtSpan::ACTIVE);

    builder.init();
}

/// Trace the given function using the given filter (in EnvFilter format).
/// Return strings representing the traced logs.
pub fn traced(filter: &str, func: impl FnOnce()) -> Vec<String> {
    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<String>>>);

    impl MakeWriter<'_> for Output {
        type Writer = Output;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    impl io::Write for Output {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut lines = self.0.lock().unwrap();
            let mut s = String::from_utf8_lossy(buf).trim().to_string();

            // Buck unittest targets add "_unittest" suffix to crate names
            // that will affect the "target". Workaround it by removing the
            // suffix.
            if cfg!(fbcode_build) {
                s = s.replace("_unittest: ", ": ");
                s = s.replace("_unittest::", "::");
            }

            lines.push(s);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let out = Output::default();
    let builder = Subscriber::builder()
        .with_env_filter(EnvFilter::new(filter))
        .with_ansi(false)
        .without_time()
        .with_writer(out.clone())
        .with_span_events(FmtSpan::ACTIVE);
    let dispatcher = builder.finish();
    tracing::subscriber::with_default(dispatcher, func);

    let lines = out.0.lock().unwrap();
    lines.clone()
}

/// Call `init` on startup. This is useful for tests.
#[macro_export]
macro_rules! init {
    () => {
        #[dev_logger::ctor]
        fn dev_logger_init_ctor() {
            dev_logger::init();
        }
    };
}

#[test]
fn test_traced() {
    let lines = traced("info", || {
        tracing::info_span!("bar", x = 1).in_scope(|| {
            tracing::info!("foo");
            tracing::debug!("foo2");
        });
    });
    assert_eq!(
        lines,
        [
            "INFO bar{x=1}: dev_logger: enter",
            "INFO bar{x=1}: dev_logger: foo",
            "INFO bar{x=1}: dev_logger: exit"
        ]
    );
}
