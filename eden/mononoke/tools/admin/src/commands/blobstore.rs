/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;
mod fetch_many;
mod upload;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use fetch::BlobstoreFetchArgs;
use fetch_many::BlobstoreFetchManyArgs;
use mononoke_app::args::RepoBlobstoreArgs;
use mononoke_app::MononokeApp;
use upload::BlobstoreUploadArgs;

/// Directly access blobstore keys
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_blobstore_args: RepoBlobstoreArgs,

    #[clap(subcommand)]
    subcommand: BlobstoreSubcommand,
}

#[derive(Subcommand)]
pub enum BlobstoreSubcommand {
    /// Fetch a blob from the blobstore
    Fetch(BlobstoreFetchArgs),
    /// Upload a blob to the blobstore
    Upload(BlobstoreUploadArgs),
    /// Fetches multiple blobs, and outputs how many were present, not present or errored.
    /// Most useful to use with scrub to repair blobs.
    FetchMany(BlobstoreFetchManyArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let blobstore = app
        .open_blobstore(&args.repo_blobstore_args)
        .await
        .context("Failed to open blobstore")?;

    match args.subcommand {
        BlobstoreSubcommand::Fetch(fetch_args) => {
            fetch::fetch(&ctx, &blobstore, fetch_args).await?
        }
        BlobstoreSubcommand::FetchMany(args) => {
            fetch_many::fetch_many(&ctx, &blobstore, args).await?
        }
        BlobstoreSubcommand::Upload(upload_args) => {
            upload::upload(&ctx, &blobstore, upload_args).await?
        }
    }

    Ok(())
}
