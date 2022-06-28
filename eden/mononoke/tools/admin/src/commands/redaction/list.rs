/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use clap::Args;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::stream::TryStreamExt;
use manifest::ManifestOps;
use metaconfig_types::RepoConfigRef;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct RedactionListArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(long, short = 'i')]
    commit_id: String,
}

/// Returns paths and content ids whose content matches the given keys in the
/// given commit
pub(super) async fn paths_for_content_keys(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    keys: &HashSet<String>,
) -> Result<Vec<(MPath, ContentId)>> {
    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;
    let file_count = root_fsnode_id
        .fsnode_id()
        .load(ctx, repo.repo_blobstore())
        .await?
        .summary()
        .descendant_files_count;
    let mut processed = 0;
    let mut paths = Vec::new();
    let mut entries = root_fsnode_id
        .fsnode_id()
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc());
    while let Some((path, fsnode_file)) = entries.try_next().await? {
        processed += 1;
        if processed % 100_000 == 0 {
            if paths.is_empty() {
                println!("Processed files: {}/{}", processed, file_count);
            } else {
                println!(
                    "Processed files: {}/{} ({} found so far)",
                    processed,
                    file_count,
                    paths.len()
                );
            }
        }
        if keys.contains(&fsnode_file.content_id().blobstore_key()) {
            paths.push((path, fsnode_file.content_id().clone()));
        }
    }
    Ok(paths)
}

pub async fn list(
    ctx: &CoreContext,
    app: &MononokeApp,
    list_args: RedactionListArgs,
) -> Result<()> {
    let repo: Repo = app
        .open_repo(&list_args.repo_args)
        .await
        .context("Failed to open repo")?;

    let cs_id = parse_commit_id(ctx, &repo, &list_args.commit_id).await?;

    // We don't have a way to get the keys for the redacted blobs out of the
    // repo blobstore, so we must ask the factory to load them again.  Until
    // SqlRedactedBlobs are removed, we need to know the metadata database
    // config for this.
    let db_config = &repo.repo_config().storage_config.metadata;
    let redacted_blobs = app
        .repo_factory()
        .redacted_blobs(ctx.clone(), db_config)
        .await?;
    let redacted_map = redacted_blobs.redacted();
    let keys = redacted_map.keys().cloned().collect();

    println!("Searching for redacted paths in {}", cs_id);
    let mut redacted_paths = paths_for_content_keys(ctx, &repo, cs_id, &keys).await?;
    println!("Found {} redacted paths", redacted_paths.len());

    redacted_paths.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content_id) in redacted_paths {
        if let Some(meta) = redacted_map.get(&content_id.blobstore_key()) {
            let log_only = if meta.log_only { " (log only)" } else { "" };
            println!("{:20}: {}{}", meta.task, path, log_only);
        }
    }

    Ok(())
}
