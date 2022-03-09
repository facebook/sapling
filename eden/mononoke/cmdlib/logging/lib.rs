/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
pub mod glog;
pub mod log;
mod logging_args;
mod scribe;
mod scuba;

pub use logging_args::LoggingArgs;
pub use scribe::ScribeLoggingArgs;
pub use scuba::ScubaLoggingArgs;
