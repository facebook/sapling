/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::Repo;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use slog::Logger;

#[derive(Args)]
/// Subcommand to describe metadata associated with a bubble within the ephemeral store.
pub struct EphemeralStoreInfoArgs {
    /// The ID of the changeset for which the bubble metadata is requested.
    #[clap(long, short = 'c')]
    changesetid: Option<u64>,

    /// The ID of the bubble for which the metadata is requested. If provided,
    /// the changeset ID is ignored.
    #[clap(long, short = 'b')]
    bubbleid: Option<u64>,
}

pub async fn bubble_info(
    _ctx: &CoreContext,
    _repo: &Repo,
    _logger: &Logger,
    _args: EphemeralStoreInfoArgs,
) -> Result<()> {
    // TODO: Implement bubble metadata fetch logic
    Ok(())
}
