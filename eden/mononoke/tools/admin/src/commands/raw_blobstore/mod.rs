/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;
mod upload;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::fetch::RawBlobstoreFetchArgs;
use self::upload::RawBlobstoreUploadArgs;

/// Directly access raw blobstore keys without repo prefix
#[derive(Parser)]
pub struct CommandArgs {
    /// Storage name
    #[clap(long, required = true)]
    storage_name: String,

    /// If the blobstore is multiplexed, use this inner blobstore
    #[clap(long)]
    inner_blobstore_id: Option<u64>,

    #[clap(subcommand)]
    subcommand: RawBlobstoreSubcommand,
}

#[derive(Subcommand)]
pub enum RawBlobstoreSubcommand {
    /// Fetch a blob from the blobstore (raw version without HgAugmentedManifest rendering)
    Fetch(RawBlobstoreFetchArgs),
    /// Upload a blob to the blobstore
    Upload(RawBlobstoreUploadArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let blobstore = app
        .open_raw_blobstore(&args.storage_name, args.inner_blobstore_id)
        .await
        .context("Failed to open raw blobstore")?;

    match args.subcommand {
        RawBlobstoreSubcommand::Fetch(fetch_args) => {
            fetch::fetch(&ctx, blobstore.as_ref(), fetch_args).await
        }
        RawBlobstoreSubcommand::Upload(upload_args) => {
            upload::upload(&ctx, blobstore.as_ref(), upload_args).await
        }
    }
}
