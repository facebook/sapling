/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for path selection.

#[derive(clap::Args, Clone)]
pub(crate) struct PathArgs {
    #[clap(long, short)]
    /// Path
    pub path: String,
}
