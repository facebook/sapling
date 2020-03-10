/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integration with the `log` eco-system.

use std::fmt;

/// Convert a `tracing::Level` to `log::Level`.
#[inline]
fn convert_level(level: tracing::Level) -> ::log::Level {
    match level {
        tracing::Level::ERROR => ::log::Level::Error,
        tracing::Level::WARN => ::log::Level::Warn,
        tracing::Level::INFO => ::log::Level::Info,
        tracing::Level::DEBUG => ::log::Level::Debug,
        tracing::Level::TRACE => ::log::Level::Trace,
    }
}

/// Whether this should be logged by the `log` eco-system.
#[inline]
pub(crate) fn enabled(metadata: &tracing::Metadata) -> bool {
    let level = convert_level(metadata.level().clone());
    let target = metadata.target(); // usually module name
    ::log::logger().enabled(
        &::log::Metadata::builder()
            .level(level)
            .target(target)
            .build(),
    )
}

/// Write a log to the log eco-system.
#[inline]
pub(crate) fn log(message: impl fmt::Display, metadata: &tracing::Metadata) {
    let level = convert_level(metadata.level().clone());
    ::log::logger().log(
        &::log::Record::builder()
            .args(format_args!("{}", message))
            .level(level)
            .target(metadata.target())
            .module_path(metadata.module_path())
            .file(metadata.file())
            .line(metadata.line())
            .build(),
    );
}

/// Wrapper of `tracing::Event` so we can implement `Display`.
pub(crate) struct DisplayEvent<'a>(pub(crate) &'a tracing::Event<'a>);
impl<'a> fmt::Display for DisplayEvent<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut padding = "";
        let mut visitor = |field: &tracing::field::Field, value: &dyn fmt::Debug| {
            let name = field.name();
            let _ = if name == "name" {
                // Remove "name=".
                write!(f, "{}{:?}", padding, value)
            } else {
                write!(f, "{}{}={:?}", padding, field.name(), value)
            };
            padding = " ";
        };
        self.0.record(&mut visitor);
        Ok(())
    }
}
