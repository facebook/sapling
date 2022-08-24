/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Commandline args to make the server read-only
#[derive(Args, Debug)]
pub struct ReadonlyArgs {
    /// Makes the server completely readonly by failing all write ACL checks.
    #[clap(long)]
    pub readonly: bool,
}
