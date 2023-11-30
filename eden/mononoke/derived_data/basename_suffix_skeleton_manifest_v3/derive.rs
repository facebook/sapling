/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use basename_suffix_skeleton_manifest::BssmPath;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Either;
use manifest::derive_manifest;
use manifest::get_implicit_deletes;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::TreeInfo;
use manifest::TreeInfoSubentries;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Directory;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Entry;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BssmV3DirectoryId;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;
use mononoke_types::TrieMap;
use skeleton_manifest::mapping::get_file_changes;
use skeleton_manifest::RootSkeletonManifestId;

use crate::mapping::RootBssmV3DirectoryId;

/// Returns the changes that need to be applied to BSSM during derivation,
/// by getting the file changes from a bonsai changeset, expanding implicit
/// deletes, then applying the bssm transform to the paths.
async fn get_bssm_path_changes(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    bcs: &BonsaiChangeset,
    parent_skeleton_manifests: Vec<SkeletonManifestId>,
) -> Result<Vec<(NonRootMPath, Option<()>)>> {
    let changes = get_file_changes(bcs)
        .into_iter()
        .map(|(path, content)| (path, content.map(|_| ())))
        .collect::<Vec<_>>();

    // Implicit deletes must be expanded before doing the bssm transform on the changes.
    // This is because paths that were prefixes of each other before the transform can
    // become independent afterwards, and the only implicit deletes after the transform
    // that are perserved are those that share the same basename. For example:
    //
    // Before bssm transform:
    // - Parent manifest has one file: a/b/c
    // - Bonsai changeset adds a file: a/b
    // - a/b/c is implicitly deleted.
    //
    // After bssm transform:
    // - Parent manifest has one file: c/a/b/c
    // - Bonsai changeset adds a file: b/a/b
    // - no implicit deletes.
    let implicit_deletes = get_implicit_deletes(
        ctx,
        blobstore.clone(),
        changes
            .iter()
            .flat_map(|(path, change)| change.map(|()| path))
            .cloned()
            .collect::<Vec<_>>(),
        parent_skeleton_manifests,
    )
    .try_collect::<Vec<_>>()
    .await?;

    Ok(changes
        .into_iter()
        .chain(implicit_deletes.into_iter().map(|path| (path, None)))
        .map(|(path, change)| (BssmPath::transform(path).into_raw(), change))
        .collect())
}

async fn empty_directory_id(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<BssmV3DirectoryId> {
    let leaf = BssmV3Directory::empty();
    leaf.into_blob().store(ctx, blobstore).await
}

fn mf_entry_to_bssm_v3_entry(entry: Entry<BssmV3Directory, ()>) -> BssmV3Entry {
    match entry {
        Entry::Leaf(()) => BssmV3Entry::File,
        Entry::Tree(entry) => BssmV3Entry::Directory(entry),
    }
}

async fn create_bssm_v3_directory(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    subentries: TreeInfoSubentries<BssmV3Directory, (), (), LoadableShardedMapV2Node<BssmV3Entry>>,
) -> Result<BssmV3Directory> {
    let subentries: TrieMap<_> = match subentries {
        TreeInfoSubentries::AllSubentries(subentries) => subentries
            .into_iter()
            .map(|(path, (_ctx, entry))| (path, Either::Left(mf_entry_to_bssm_v3_entry(entry))))
            .collect(),
        TreeInfoSubentries::ReusedMapsAndSubentries {
            produced_subentries_and_reused_maps,
            ..
        } => produced_subentries_and_reused_maps
            .into_iter()
            .map(|(prefix, entry_or_map)| match entry_or_map {
                Either::Left((_maybe_ctx, entry)) => {
                    (prefix, Either::Left(mf_entry_to_bssm_v3_entry(entry)))
                }
                Either::Right(map) => (prefix, Either::Right(map)),
            })
            .collect(),
    };

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, subentries).await?;

    Ok(BssmV3Directory { subentries })
}

pub(crate) async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<BssmV3Directory>,
    changes: Vec<(NonRootMPath, Option<()>)>,
) -> Result<Option<BssmV3Directory>> {
    type Leaf = ();
    type LeafId = ();
    type TreeId = BssmV3Directory;
    type IntermediateLeafId = LeafId;
    type Ctx = ();

    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        {
            cloned!(ctx, blobstore);
            move |info: TreeInfo<
                TreeId,
                IntermediateLeafId,
                Ctx,
                LoadableShardedMapV2Node<BssmV3Entry>,
            >| {
                cloned!(ctx, blobstore);
                async move {
                    let directory =
                        create_bssm_v3_directory(ctx, blobstore, info.subentries)
                            .await?;
                    Ok(((), directory))
                }
            }
        },
        {
            move |_leaf_info: LeafInfo<IntermediateLeafId, Leaf>| async move {
                anyhow::Ok(((), ()))
            }
        },
    )
    .await
}

pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootBssmV3DirectoryId>,
    parent_skeleton_manifests: Vec<RootSkeletonManifestId>,
) -> Result<RootBssmV3DirectoryId> {
    let parent_skeleton_manifests = parent_skeleton_manifests
        .into_iter()
        .map(|parent_skeleton_manifest| parent_skeleton_manifest.into_skeleton_manifest_id())
        .collect::<Vec<_>>();

    let blobstore = derivation_ctx.blobstore();
    let changes =
        get_bssm_path_changes(ctx, blobstore.clone(), &bonsai, parent_skeleton_manifests).await?;

    let parents = stream::iter(parents)
        .map(|parent| async move { parent.0.load(ctx, blobstore).await })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let root_directory = inner_derive(ctx, blobstore, parents, changes).await?;

    Ok(RootBssmV3DirectoryId(match root_directory {
        Some(directory) => {
            let blob = directory.into_blob();
            blob.store(ctx, blobstore)
                .await
                .context("failed to store BssmV3Directory blob")?
        }
        None => empty_directory_id(ctx, blobstore).await?,
    }))
}
