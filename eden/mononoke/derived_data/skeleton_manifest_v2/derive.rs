/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use itertools::Either;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use manifest::TreeInfo;
use manifest::TreeInfoSubentries;
use manifest::derive_manifest;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestV2Id;
use mononoke_types::TrieMap;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2Entry;

use crate::RootSkeletonManifestV2Id;

fn get_file_changes(bcs: &BonsaiChangeset) -> Vec<(NonRootMPath, Option<()>)> {
    bcs.file_changes()
        .map(|(path, file_change)| (path.clone(), file_change.simplify().map(|_| ())))
        .collect()
}

async fn get_skeleton_manifest_subtree_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, RootSkeletonManifestV2Id>>,
    bcs: &BonsaiChangeset,
) -> Result<Vec<ManifestParentReplacement<SkeletonManifestV2, ()>>> {
    let copy_sources = bcs
        .subtree_changes()
        .iter()
        .filter_map(|(path, change)| {
            let (from_cs_id, from_path) = change.copy_source()?;
            Some((path, from_cs_id, from_path))
        })
        .collect::<Vec<_>>();
    stream::iter(copy_sources)
        .map(|(path, from_cs_id, from_path)| {
            cloned!(ctx);
            let blobstore = derivation_ctx.blobstore().clone();
            async move {
                let root = derivation_ctx
                    .fetch_unknown_dependency::<RootSkeletonManifestV2Id>(&ctx, known, from_cs_id)
                    .await?
                    .into_inner_id()
                    .load(&ctx, &blobstore)
                    .await?;
                let entry = root
                    .find_entry(ctx, blobstore, from_path.clone())
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Subtree copy source {} does not exist in {}",
                            from_path,
                            from_cs_id
                        )
                    })?;
                Ok(ManifestParentReplacement {
                    path: path.clone(),
                    replacements: vec![entry],
                })
            }
        })
        .buffered(100)
        .try_collect()
        .boxed()
        .await
}

async fn empty_manifest_id(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<SkeletonManifestV2Id> {
    let empty_manifest = SkeletonManifestV2::empty();
    empty_manifest.into_blob().store(ctx, blobstore).await
}

fn mf_entry_to_skeleton_manifest_v2_entry(
    entry: Entry<SkeletonManifestV2, ()>,
) -> SkeletonManifestV2Entry {
    match entry {
        Entry::Leaf(()) => SkeletonManifestV2Entry::File,
        Entry::Tree(dir) => SkeletonManifestV2Entry::Directory(dir),
    }
}

async fn create_skeleton_manifest_v2(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    subentries: TreeInfoSubentries<
        SkeletonManifestV2,
        (),
        (),
        LoadableShardedMapV2Node<SkeletonManifestV2Entry>,
    >,
) -> Result<SkeletonManifestV2> {
    let subentries: TrieMap<_> = subentries
        .into_iter()
        .map(|(prefix, entry_or_map)| match entry_or_map {
            Either::Left((_maybe_ctx, entry)) => (
                prefix,
                Either::Left(mf_entry_to_skeleton_manifest_v2_entry(entry)),
            ),
            Either::Right(map) => (prefix, Either::Right(map)),
        })
        .collect();

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, subentries).await?;

    Ok(SkeletonManifestV2 { subentries })
}

pub(crate) async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<SkeletonManifestV2>,
    changes: Vec<(NonRootMPath, Option<()>)>,
    subtree_changes: Vec<ManifestParentReplacement<SkeletonManifestV2, ()>>,
) -> Result<Option<SkeletonManifestV2>> {
    type Leaf = ();
    type LeafChange = ();
    type TreeId = SkeletonManifestV2;
    type Ctx = ();

    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        subtree_changes,
        {
            cloned!(ctx, blobstore);
            move |info: TreeInfo<
                TreeId,
                Leaf,
                Ctx,
                LoadableShardedMapV2Node<SkeletonManifestV2Entry>,
            >| {
                cloned!(ctx, blobstore);
                async move {
                    let manifest =
                        create_skeleton_manifest_v2(ctx, blobstore, info.subentries).await?;
                    Ok(((), manifest))
                }
            }
        },
        move |_leaf_info: LeafInfo<Leaf, LeafChange>| async move { anyhow::Ok(((), ())) },
    )
    .await
}

pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootSkeletonManifestV2Id>,
    known: Option<&HashMap<ChangesetId, RootSkeletonManifestV2Id>>,
) -> Result<RootSkeletonManifestV2Id> {
    let blobstore = derivation_ctx.blobstore();
    let changes = get_file_changes(&bonsai);
    let subtree_changes =
        get_skeleton_manifest_subtree_changes(ctx, derivation_ctx, known, &bonsai).await?;

    let parents = stream::iter(parents)
        .map(|parent| async move { parent.0.load(ctx, blobstore).await })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let root_manifest = inner_derive(ctx, blobstore, parents, changes, subtree_changes).await?;

    Ok(RootSkeletonManifestV2Id(match root_manifest {
        Some(manifest) => {
            let blob = manifest.into_blob();
            blob.store(ctx, blobstore)
                .await
                .context("failed to store SkeletonManifestV2 blob")?
        }
        None => empty_manifest_id(ctx, blobstore).await?,
    }))
}
