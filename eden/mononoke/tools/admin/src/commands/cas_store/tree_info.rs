/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use clap::Args;
use context::CoreContext;
use futures::StreamExt;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mononoke_types::sha1_hash::Sha1;
use repo_blobstore::RepoBlobstoreRef;

use super::Repo;

#[derive(Args)]
pub struct CasStoreTreeInfoArgs {
    /// hgid of the tree
    #[clap(long, short = 'i')]
    hgid: Sha1,

    /// Show information for each entry in the tree
    #[clap(long, short = 'c')]
    show_entries: bool,
}

pub async fn tree_info(ctx: &CoreContext, repo: &Repo, args: CasStoreTreeInfoArgs) -> Result<()> {
    let augmented_manifest_id = HgAugmentedManifestId::from_sha1(args.hgid);
    let augmented_manifest = augmented_manifest_id
        .load(ctx, repo.repo_blobstore())
        .await?;

    println!(
        "CAS digest: {}:{}",
        augmented_manifest.augmented_manifest_id, augmented_manifest.augmented_manifest_size
    );

    if args.show_entries {
        let mut entries_stream = augmented_manifest
            .augmented_manifest
            .into_subentries(ctx, repo.repo_blobstore());
        while let Some(result) = entries_stream.next().await {
            let (name, entry) = result?;
            match entry {
                HgAugmentedManifestEntry::FileNode(file) => {
                    println!(
                        "{} -> File CAS digest: {}:{} HgId: {}",
                        name, file.content_blake3, file.total_size, file.filenode,
                    );
                }
                HgAugmentedManifestEntry::DirectoryNode(tree) => {
                    println!(
                        "{} -> Tree CAS digest: {}:{} HgId: {}",
                        name,
                        tree.augmented_manifest_id,
                        tree.augmented_manifest_size,
                        tree.treenode,
                    );
                }
            }
        }
    }

    Ok(())
}
