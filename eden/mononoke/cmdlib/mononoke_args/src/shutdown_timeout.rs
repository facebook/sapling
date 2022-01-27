/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use std::time::Duration;

/// Command line arguments for shutdown timeout
#[derive(Args, Debug)]
pub struct ShutdownTimeoutArgs {
    /// Number of seconds to wait after receiving a shutdown signal before shutting down
    #[clap(long, default_value = "0", parse(try_from_str=duration_secs_from_str))]
    pub shutdown_grace_period: Duration,

    /// Number of seconds to wait for requests to complete during shutdown
    #[clap(long, default_value = "10", parse(try_from_str=duration_secs_from_str))]
    pub shutdown_timeout: Duration,
}

fn duration_secs_from_str(s: &str) -> Result<Duration> {
    Ok(Duration::from_secs(s.parse::<u64>()?))
}
