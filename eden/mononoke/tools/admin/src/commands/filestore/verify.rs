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
use filestore::Alias;
use filestore::FetchKey;
use repo_blobstore::RepoBlobstoreRef;

use super::FilestoreItemIdArgs;
use super::Repo;

#[derive(Args)]
pub struct FilestoreVerifyArgs {
    #[clap(flatten)]
    item_id: FilestoreItemIdArgs,

    #[clap(long)]
    bubble_id: Option<BubbleId>,

    #[clap(long, short = 'o', value_name = "FILE", parse(from_os_str))]
    output: Option<PathBuf>,
}

pub async fn verify(
    ctx: &CoreContext,
    repo: &Repo,
    verify_args: FilestoreVerifyArgs,
) -> Result<()> {
    let fetch_key = verify_args.item_id.fetch_key()?;
    let blobstore = match verify_args.bubble_id {
        Some(bubble_id) => {
            let bubble = repo.repo_ephemeral_store().open_bubble(bubble_id).await?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
        None => repo.repo_blobstore().clone(),
    };
    let metadata = filestore::get_metadata(&blobstore, ctx, &fetch_key)
        .await
        .context("Failed to get metadata from filestore")?
        .ok_or_else(|| anyhow!("Content not found"))?;

    let (content_id, sha1, sha256, git_sha1) = futures::future::join4(
        filestore::fetch(
            &blobstore,
            ctx.clone(),
            &FetchKey::Canonical(metadata.content_id),
        ),
        filestore::fetch(
            &blobstore,
            ctx.clone(),
            &FetchKey::Aliased(Alias::Sha1(metadata.sha1)),
        ),
        filestore::fetch(
            &blobstore,
            ctx.clone(),
            &FetchKey::Aliased(Alias::Sha256(metadata.sha256)),
        ),
        filestore::fetch(
            &blobstore,
            ctx.clone(),
            &FetchKey::Aliased(Alias::GitSha1(metadata.git_sha1.sha1())),
        ),
    )
    .await;

    println!("content_id: {}", content_id.is_ok());
    println!("sha1: {}", sha1.is_ok());
    println!("sha256: {}", sha256.is_ok());
    println!("git_sha1: {}", git_sha1.is_ok());

    Ok(())
}
