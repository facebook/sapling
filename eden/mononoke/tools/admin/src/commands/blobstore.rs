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
use mononoke_app::args::RepoBlobstoreArgs;
use mononoke_app::MononokeApp;

use fetch::BlobstoreFetchArgs;
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
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let blobstore = app
        .open_blobstore(&args.repo_blobstore_args)
        .await
        .context("Failed to open blobstore")?;

    match args.subcommand {
        BlobstoreSubcommand::Fetch(fetch_args) => {
            fetch::fetch(&ctx, &blobstore, fetch_args).await?
        }
        BlobstoreSubcommand::Upload(upload_args) => {
            upload::upload(&ctx, &blobstore, upload_args).await?
        }
    }

    Ok(())
}
