/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod args;
mod builder;
pub mod log;

pub use args::{LoggingArgs, ScubaLoggingArgs};
#[cfg(fbcode_build)]
pub use builder::set_glog_log_level;
pub use builder::{
    create_log_level, create_logger, create_observability_context, create_root_log_drain,
    create_scuba_sample_builder, create_warm_bookmark_cache_scuba_sample_builder,
};
