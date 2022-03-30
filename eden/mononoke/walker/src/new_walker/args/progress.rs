/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;
use std::time::Duration;
use walker_commands_impl::progress::ProgressOptions;
use walker_commands_impl::setup::{PROGRESS_SAMPLE_DURATION_S, PROGRESS_SAMPLE_RATE};

#[derive(Args, Debug)]
pub struct ProgressArgs {
    /// Minimum interval between progress reports in seconds.
    #[clap(long, default_value_t = PROGRESS_SAMPLE_DURATION_S)]
    pub progress_interval: u64,
    /// Sample the walk output stream for progress roughly 1 in N steps.
    /// Only log if progress-interval has passed.
    #[clap(long, default_value_t = PROGRESS_SAMPLE_RATE)]
    pub progress_sample_rate: u64,
}

impl ProgressArgs {
    #[allow(dead_code)]
    pub fn parse_args(&self) -> ProgressOptions {
        ProgressOptions {
            sample_rate: self.progress_sample_rate,
            interval: Duration::from_secs(self.progress_interval),
        }
    }
}
