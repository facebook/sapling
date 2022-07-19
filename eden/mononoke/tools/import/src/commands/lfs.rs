/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use borrowed::borrowed;
use bytes::Bytes;
use clap::Parser;
use filestore::FilestoreConfig;
use futures::stream;
use futures::TryStreamExt;
use lfs_import_lib::lfs_upload;
use mercurial_types::blobs::File;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;

/// Import LFS blobs
#[derive(Parser)]
pub struct CommandArgs {
    /// LFS Helper
    lfs_helper: String,

    /// Raw LFS pointers to be imported
    #[clap(required(true))]
    pointers: Vec<String>,

    #[clap(flatten)]
    repo_args: RepoArgs,

    /// The number of OIDs to process in parallel
    #[clap(long, default_value = "16")]
    concurrency: usize,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    filestore_config: FilestoreConfig,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let entries: Vec<_> = args
        .pointers
        .into_iter()
        .map(|e| File::new(Bytes::copy_from_slice(e.as_bytes()), None, None).get_lfs_content())
        .collect();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    let ctx = app.new_context();

    stream::iter(entries)
        .try_for_each_concurrent(args.concurrency, {
            borrowed!(ctx, repo, args.lfs_helper);
            move |lfs| async move {
                lfs_upload(ctx, repo, lfs_helper, &lfs).await?;
                Ok(())
            }
        })
        .await?;

    Ok(())
}
