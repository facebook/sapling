/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod args;
mod builder;
pub mod log;

pub use args::LoggingArgs;
pub use builder::{create_log_level, create_root_log_drain};
