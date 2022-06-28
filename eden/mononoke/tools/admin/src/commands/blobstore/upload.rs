/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use clap::Args;
use context::CoreContext;

#[derive(Args)]
pub struct BlobstoreUploadArgs {
    /// Read raw blob bytes from the given filename
    #[clap(long, value_name = "FILE")]
    value_file: PathBuf,

    /// Blobstore key to fetch.
    #[clap(long)]
    key: String,
}

pub async fn upload(
    ctx: &CoreContext,
    blobstore: &dyn Blobstore,
    upload_args: BlobstoreUploadArgs,
) -> Result<()> {
    let data = tokio::fs::read(upload_args.value_file)
        .await
        .context("Failed to read value file")?;

    writeln!(
        std::io::stdout(),
        "Writing {} bytes to blobstore key {}",
        data.len(),
        upload_args.key
    )?;

    blobstore
        .put(ctx, upload_args.key, BlobstoreBytes::from_bytes(data))
        .await
        .context("Failed to upload blob")?;

    Ok(())
}
