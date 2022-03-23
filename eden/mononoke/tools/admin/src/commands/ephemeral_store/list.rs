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

// By default, at most 100 blob keys will be listed for a bubble.
const DEFAULT_MAX_KEYS_FOR_LIST: &str = "100";

#[derive(Args)]
/// Subcommand to list out the keys of the blobs in a bubble within the ephemeral store.
pub struct EphemeralStoreListArgs {
    /// The ID of the changeset for which the bubble blob data needs to be listed.
    #[clap(long, short = 'c')]
    changesetid: Option<u64>,

    /// The ID of the bubble for which the blob data needs to be listed. If provided,
    /// the changeset ID is ignored.
    #[clap(long, short = 'b')]
    bubbleid: Option<u64>,

    /// The maximum number of blob keys listed in the output. Defaults to 100.
    #[clap(long, short = 'l', default_value = DEFAULT_MAX_KEYS_FOR_LIST)]
    limit: u32,

    /// If specified, the search range starts from this key.
    #[clap(long)]
    after: Option<String>,
}

pub async fn list_keys(
    _ctx: &CoreContext,
    _repo: &Repo,
    _logger: &Logger,
    _args: EphemeralStoreListArgs,
) -> Result<()> {
    // TODO: Implement bubble keys list logic
    Ok(())
}
