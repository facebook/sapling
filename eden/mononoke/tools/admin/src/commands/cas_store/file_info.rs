/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use clap::Args;
use context::CoreContext;
use filestore::FetchKey;
use mercurial_types::HgFileNodeId;
use mononoke_types::sha1_hash::Sha1;
use repo_blobstore::RepoBlobstoreRef;

use super::Repo;

#[derive(Args)]
pub struct CasStoreFileInfoArgs {
    /// hgid of the file node
    #[clap(long, short = 'i')]
    hgid: Sha1,
}

pub async fn file_info(ctx: &CoreContext, repo: &Repo, args: CasStoreFileInfoArgs) -> Result<()> {
    let file_node_id = HgFileNodeId::from_sha1(args.hgid);
    let file_node = file_node_id.load(ctx, repo.repo_blobstore()).await?;
    let metadata = filestore::get_metadata(
        repo.repo_blobstore(),
        ctx,
        &FetchKey::from(file_node.content_id()),
    )
    .await?
    .ok_or_else(|| anyhow!("Content not found"))?;

    println!(
        "CAS digest: {}:{}",
        metadata.seeded_blake3, metadata.total_size,
    );

    Ok(())
}
