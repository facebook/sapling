/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod copy_keys;
mod fetch;
mod fetch_many;
mod upload;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use copy_keys::BlobstoreCopyKeysArgs;
use fetch::BlobstoreFetchArgs;
use fetch_many::BlobstoreFetchManyArgs;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoBlobstoreArgs;
use upload::BlobstoreUploadArgs;

/// Directly access blobstore keys
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_blobstore_args: Option<RepoBlobstoreArgs>,

    #[clap(subcommand)]
    subcommand: BlobstoreSubcommand,
}

#[derive(Subcommand)]
pub enum BlobstoreSubcommand {
    /// Copy a list of blobs from one blobstore to another
    CopyKeys(BlobstoreCopyKeysArgs),
    /// Fetch a blob from the blobstore
    Fetch(BlobstoreFetchArgs),
    /// Upload a blob to the blobstore
    Upload(BlobstoreUploadArgs),
    /// Fetches multiple blobs, and outputs how many were present, not present or errored.
    /// Most useful to use with scrub to repair blobs.
    FetchMany(BlobstoreFetchManyArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    match args.subcommand {
        BlobstoreSubcommand::CopyKeys(copy_keys_args) => {
            copy_keys::copy_keys(app, copy_keys_args).await
        }
        subcommand => {
            let repo_blobstore_args = args
                .repo_blobstore_args
                .context("repo_blobstore_args required for this command")?;
            let ctx = app.new_basic_context();
            let blobstore = app
                .open_blobstore(&repo_blobstore_args)
                .await
                .context("Failed to open blobstore")?;

            match subcommand {
                BlobstoreSubcommand::Fetch(fetch_args) => {
                    fetch::fetch(&ctx, &blobstore, fetch_args).await
                }
                BlobstoreSubcommand::FetchMany(fetch_many_args) => {
                    fetch_many::fetch_many(&ctx, &blobstore, fetch_many_args).await
                }
                BlobstoreSubcommand::Upload(upload_args) => {
                    upload::upload(&ctx, &blobstore, upload_args).await
                }
                BlobstoreSubcommand::CopyKeys(_) => unreachable!(),
            }
        }
    }
}
