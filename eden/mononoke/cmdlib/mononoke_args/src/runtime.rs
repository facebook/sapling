/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for controlling the runtime
// Defaults are derived from `sql_ext::facebook::mysql`
// https://fburl.com/diffusion/n5isd68j, last synced on 17/12/2020
#[derive(Args, Debug)]
pub struct RuntimeArgs {
    /// Number of threads to use in the Tokio runtime
    #[clap(long)]
    pub runtime_threads: Option<usize>,
}
