/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use restricted_paths::manifest_id_store::RestrictedPathsManifestIdStoreRef;

use super::Repo;
use super::parse_manifest_id;

#[derive(Args)]
pub struct ListArgs {
    /// The manifest id to look up, in hex (an optional `0x` prefix is accepted).
    #[clap(long)]
    manifest_id: String,
}

pub async fn list(ctx: &CoreContext, repo: &Repo, args: ListArgs) -> Result<()> {
    let manifest_id = parse_manifest_id(&args.manifest_id)?;
    let store = repo.restricted_paths_manifest_id_store();
    let entries = store
        .get_all_paths_by_manifest_id(ctx, &manifest_id)
        .await?;

    if entries.is_empty() {
        println!("No entries found for manifest_id {manifest_id}");
        return Ok(());
    }

    println!("manifest_type\tpath");
    for (manifest_type, path) in entries {
        println!("{manifest_type}\t{path}");
    }
    Ok(())
}
