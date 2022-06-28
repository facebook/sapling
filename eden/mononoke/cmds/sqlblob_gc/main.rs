/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use cmdlib_logging::ScribeLoggingArgs;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod commands;
mod utils;

#[derive(Parser)]
#[clap(
    name = "SQLblob GC",
    about = "Perform garbage collection on a set of SQLblob shards"
)]
pub struct MononokeSQLBlobGCArgs {
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// The name of the storage config to GC. This *must* be an XDB storage config,
    /// or a multiplex containing an XDB (in which case, give the inner blobstore ID, too
    #[clap(long)]
    storage_config_name: String,
    /// If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id
    #[clap(long)]
    inner_blobstore_id: Option<u64>,
    /// Maximum number of parallel keys to GC.  Default 100.
    #[clap(long, default_value_t = 100)]
    scheduled_max: usize,
    /// Metadata shard number to start at (or 0 if not specified)
    #[clap(long, default_value_t = 0)]
    start_shard: usize,
    /// Number of shards to walk (or all shards up to the maximum shard number if not specified
    #[clap(long)]
    shard_count: Option<usize>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    MononokeAppBuilder::new(fb)
        .build_with_subcommands::<MononokeSQLBlobGCArgs>(commands::subcommands())?
        .run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
