/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use changesets_creation::save_changesets;
use clap::Args;
use commit_id::parse_commit_id;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use sorted_vector_map::SortedVectorMap;

use super::Repo;

#[derive(Args)]
pub struct CommitDeleteDirectoryArgs {
    /// Commit ID of the parent commit (e.g. bookmark name, bonsai hash)
    #[clap(long)]
    parent: String,

    /// Path of the directory to delete
    #[clap(long)]
    path: String,

    /// Commit message for the deletion commit
    #[clap(
        long,
        default_value = "Deleted via mononoke_admin commit delete-directory"
    )]
    message: String,

    /// Author of the deletion commit
    #[clap(long, default_value = "svcscm")]
    author: String,
}

pub async fn delete_directory(
    ctx: &CoreContext,
    repo: &Repo,
    args: CommitDeleteDirectoryArgs,
) -> Result<()> {
    let parent_cs_id = parse_commit_id(ctx, repo, &args.parent).await?;

    // Derive skeleton manifest for parent
    let root_sk_id = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestId>(ctx, parent_cs_id, DerivationPriority::LOW)
        .await?
        .into_skeleton_manifest_id();

    let dir_path = NonRootMPath::new(&args.path)?;

    // Find the directory entry in the skeleton manifest
    let entry = root_sk_id
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            dir_path.clone().into(),
        )
        .await?;

    let tree_id = match entry {
        Some(Entry::Tree(tree_id)) => tree_id,
        Some(Entry::Leaf(_)) => bail!("Path '{}' is a file, not a directory", args.path),
        None => bail!("Path '{}' does not exist in the given commit", args.path),
    };

    // List all files under the directory
    let files: Vec<NonRootMPath> = tree_id
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .map_ok(|(relative_path, ())| dir_path.join(&relative_path))
        .try_collect()
        .await?;

    if files.is_empty() {
        bail!("No files found under '{}'", args.path);
    }

    println!("Deleting {} files under '{}'", files.len(), args.path);

    // Build deletion file changes
    let file_changes: SortedVectorMap<NonRootMPath, FileChange> = files
        .into_iter()
        .map(|path| (path, FileChange::Deletion))
        .collect();

    // Create the deletion commit
    let bcs = BonsaiChangesetMut {
        parents: vec![parent_cs_id],
        author: args.author,
        author_date: DateTime::now(),
        message: args.message,
        file_changes,
        ..Default::default()
    }
    .freeze()?;

    let cs_id = bcs.get_changeset_id();
    save_changesets(ctx, repo, vec![bcs]).await?;

    println!("{cs_id}");

    Ok(())
}
