/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::Subcommand;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::StoreLoadable;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest::SkeletonManifestEntry;
use mononoke_types::ChangesetId;
use mononoke_types::SkeletonManifestId;
use repo_blobstore::RepoBlobstoreRef;
use skeleton_manifest::RootSkeletonManifestId;

use super::Repo;

#[derive(Args)]
struct SkeletonManifestListArgs {
    #[clap(long)]
    recursive: bool,
}

/// Supported manifest types
#[derive(Subcommand)]
enum ListManifestSubcommand {
    SkeletonManifests(SkeletonManifestListArgs),
    // Fsnodes, // TODO(T175880214): Add support for fsnodes
    // Unodes, // TODO(T175880214): Add support for unodes
    // DeletedManifests, // TODO(T175880214): Add support for deleted manifests
}

#[derive(Args)]
pub(super) struct ListManifestsArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    /// Path you want to examine
    #[clap(long, short = 'p')]
    path: String,
    #[clap(subcommand)]
    subcommand: ListManifestSubcommand,
}

async fn skeleton_manifest_list(
    ctx: &CoreContext,
    repo: &Repo,
    path: MPath,
    skeleton_id: SkeletonManifestId,
) -> Result<()> {
    let entry = skeleton_id
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
        .await?
        .ok_or(anyhow!("Couldn't find manifest for the given path"))?;

    match entry {
        Entry::Tree(dir_skeleton_id) => {
            let dir_manifest =
                StoreLoadable::load(&dir_skeleton_id, ctx, repo.repo_blobstore()).await?;

            let subentries = dir_manifest.list();
            subentries
                .into_iter()
                .for_each(|(p, subentry)| match subentry {
                    SkeletonManifestEntry::Directory(..) => {
                        println!("{}/", path.join(p));
                    }
                    SkeletonManifestEntry::File => {
                        println!("{}", path.join(p));
                    }
                });
            Ok(())
        }
        Entry::Leaf(_) => Ok(()),
    }
}

async fn skeleton_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    path: MPath,
    recursive: bool,
) -> Result<()> {
    let root_skeleton_id = RootSkeletonManifestId::derive(ctx, repo, cs_id).await?;

    let skeleton_id = *root_skeleton_id.skeleton_manifest_id();

    if recursive {
        let entries = if let Some(path) = path.into_optional_non_root_path() {
            skeleton_id.list_leaf_entries_under(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                vec![path],
            )
        } else {
            skeleton_id.list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        };

        entries
            .try_for_each(|(path, _entry)| async move {
                println!("{}", path);
                Ok(())
            })
            .await?;

        return Ok(());
    }

    skeleton_manifest_list(ctx, repo, path, skeleton_id).await
}

pub(super) async fn list_manifests(
    ctx: &CoreContext,
    repo: &Repo,
    args: ListManifestsArgs,
) -> Result<()> {
    let cs_id = args
        .changeset_args
        .resolve_changeset(ctx, repo)
        .await?
        .ok_or_else(|| anyhow!("Changeset does not exist in this repository"))?;

    let path: MPath =
        MPath::new(args.path).context("Failed to construct MPath from provided path string")?;

    match &args.subcommand {
        ListManifestSubcommand::SkeletonManifests(skeleton_args) => {
            skeleton_manifest(ctx, repo, cs_id, path.clone(), skeleton_args.recursive).await?;
        }
    };

    Ok(())
}
