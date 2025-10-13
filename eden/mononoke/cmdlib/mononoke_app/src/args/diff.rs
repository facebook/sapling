/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;
use environment::RemoteDiffOptions;

/// Command line arguments for controlling diff routing
#[derive(Args, Debug, Clone)]
pub struct RemoteDiffArgs {
    /// Diff remotely using the diff service
    #[clap(long)]
    pub diff_remotely: bool,
}

impl From<RemoteDiffArgs> for RemoteDiffOptions {
    fn from(args: RemoteDiffArgs) -> Self {
        RemoteDiffOptions {
            diff_remotely: args.diff_remotely,
        }
    }
}
