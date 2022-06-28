/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use futures::TryStreamExt;
use repo_blobstore::RepoBlobstoreRef;
use tokio::io::AsyncWriteExt;
use tokio::io::BufWriter;

use super::FilestoreItemIdArgs;
use super::Repo;

#[derive(Args)]
pub struct FilestoreFetchArgs {
    #[clap(flatten)]
    item_id: FilestoreItemIdArgs,

    #[clap(long)]
    bubble_id: Option<BubbleId>,

    #[clap(long, short = 'o', value_name = "FILE", parse(from_os_str))]
    output: Option<PathBuf>,
}

pub async fn fetch(ctx: &CoreContext, repo: &Repo, fetch_args: FilestoreFetchArgs) -> Result<()> {
    let fetch_key = fetch_args.item_id.fetch_key()?;
    let blobstore = match fetch_args.bubble_id {
        Some(bubble_id) => {
            let bubble = repo.repo_ephemeral_store().open_bubble(bubble_id).await?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
        None => repo.repo_blobstore().clone(),
    };
    let mut stream = filestore::fetch(blobstore, ctx.clone(), &fetch_key)
        .await
        .context("Failed to fetch from filestore")?
        .ok_or_else(|| anyhow!("Content not found"))?;

    match fetch_args.output {
        Some(path) => {
            let file = tokio::fs::File::create(path).await?;
            let mut out = BufWriter::new(file);
            while let Some(b) = stream
                .try_next()
                .await
                .context("Failed to read from filestore")?
            {
                out.write_all(b.as_ref())
                    .await
                    .context("Failed to write to file")?;
            }
            out.shutdown()
                .await
                .context("Failed to finish writing to file")?;
        }
        None => {
            let mut out = tokio::io::stdout();
            while let Some(b) = stream
                .try_next()
                .await
                .context("Failed to read from filestore")?
            {
                out.write_all(b.as_ref())
                    .await
                    .context("Failed to write to output")?;
            }
        }
    }

    Ok(())
}
