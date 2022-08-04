/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use clap::Args;

use crate::detail::progress::ProgressOptions;

#[derive(Args, Debug)]
pub struct ProgressArgs {
    /// Minimum interval between progress reports in seconds.
    #[clap(long, default_value_t = 5)]
    pub progress_interval: u64,
    /// Sample the walk output stream for progress roughly 1 in N steps.
    /// Only log if progress-interval has passed.
    #[clap(long, default_value_t = 100)]
    pub progress_sample_rate: u64,
}

impl ProgressArgs {
    pub fn parse_args(&self) -> ProgressOptions {
        ProgressOptions {
            sample_rate: self.progress_sample_rate,
            interval: Duration::from_secs(self.progress_interval),
        }
    }
}
