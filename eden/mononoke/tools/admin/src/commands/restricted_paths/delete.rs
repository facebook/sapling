/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use restricted_paths::manifest_id_store::RestrictedPathsManifestIdStoreRef;

use super::Repo;
use super::parse_manifest_id;

#[derive(Args)]
pub struct DeleteArgs {
    /// The manifest id to delete, in hex (an optional `0x` prefix is accepted).
    #[clap(long)]
    manifest_id: String,
    /// Skip the interactive confirmation prompt.
    #[clap(long)]
    force: bool,
}

pub async fn delete(ctx: &CoreContext, repo: &Repo, args: DeleteArgs) -> Result<()> {
    let manifest_id = parse_manifest_id(&args.manifest_id)?;
    let store = repo.restricted_paths_manifest_id_store();
    let entries = store
        .get_all_paths_by_manifest_id(ctx, &manifest_id)
        .await?;

    if entries.is_empty() {
        println!("No entries found for manifest_id {manifest_id}; nothing to delete.");
        return Ok(());
    }

    let expected = entries.len();
    println!("Found {expected} entries for manifest_id {manifest_id}:");
    println!("manifest_type\tpath");
    for (manifest_type, path) in &entries {
        println!("{manifest_type}\t{path}");
    }

    if !args.force {
        // The prompt reads from stdin, which would block the tokio runtime, so
        // run it on the blocking pool.
        let confirmed = tokio::task::spawn_blocking(move || -> Result<bool> {
            print!(
                "Are you sure you want to delete {expected} manifest entries from the DB? [y/N]: "
            );
            std::io::stdout().flush()?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let answer = input.trim().to_lowercase();
            Ok(answer == "y" || answer == "yes")
        })
        .await??;

        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let deleted = store.delete_by_manifest_id(ctx, &manifest_id).await?;
    println!("Deleted {deleted} rows.");
    if deleted != expected as u64 {
        println!(
            "Warning: deleted {deleted} rows but {expected} were shown in the preview; the table changed between listing and deletion."
        );
    }
    Ok(())
}
