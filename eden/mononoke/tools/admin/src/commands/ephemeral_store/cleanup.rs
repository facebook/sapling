/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::repo::AdminRepo;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use slog::Logger;

// By default, at most 50 expired bubbles will be cleaned up in one go.
const DEFAULT_MAX_BUBBLES_FOR_CLEANUP: &str = "50";

#[derive(Args)]
/// Subcommand to cleanup expired bubbles within the ephemeral store.
pub struct EphemeralStoreCleanUpArgs {
    /// Duration in seconds for which the bubbles should already
    /// be expired.
    #[clap(long, short = 'c')]
    cutoff: u64,

    /// The maximum number of bubbles that can be cleaned up in
    /// one run of the command.
    #[clap(long, short = 'm', default_value = DEFAULT_MAX_BUBBLES_FOR_CLEANUP)]
    max: u32,

    /// When set, the command won't actually cleanup the bubbles but
    /// instead just lists the bubble IDs that will be cleaned-up on
    /// a non-dryrun of this command.
    #[clap(long, short = 'n')]
    dryrun: bool,
}

pub async fn clean_bubbles(
    _ctx: &CoreContext,
    _repo: &AdminRepo,
    _logger: &Logger,
    _args: EphemeralStoreCleanUpArgs,
) -> Result<()> {
    // TODO: Implement bubble clean up logic
    Ok(())
}
