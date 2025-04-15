/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! See docs/basename_suffix_skeleton_manifest.md for more information
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use blobstore::Blobstore;
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
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use manifest::TreeInfoSubentries;
use manifest::derive_manifest;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::test_sharded_manifest::TestShardedManifest;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::test_sharded_manifest::TestShardedManifestFile;
use skeleton_manifest::mapping::get_file_changes;

use crate::mapping::RootTestShardedManifestDirectory;

async fn empty_directory(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<TestShardedManifestDirectory> {
    let leaf = TestShardedManifest::empty();
    let id = leaf.into_blob().store(ctx, blobstore).await?;

    Ok(TestShardedManifestDirectory {
        id,
        max_basename_length: Default::default(),
    })
}

fn get_changes(bcs: &BonsaiChangeset) -> Vec<(NonRootMPath, Option<()>)> {
    get_file_changes(bcs)
        .into_iter()
        .map(|(path, content)| (path, content.map(|_| ())))
        .collect()
}

pub async fn get_test_sharded_manifest_subtree_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, RootTestShardedManifestDirectory>>,
    bcs: &BonsaiChangeset,
) -> Result<Vec<ManifestParentReplacement<TestShardedManifestDirectory, ()>>> {
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
                    .fetch_unknown_dependency::<RootTestShardedManifestDirectory>(
                        &ctx, known, from_cs_id,
                    )
                    .await?
                    .into_inner();
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

fn mf_entry_to_test_sharded_mf_entry(
    entry: Entry<TestShardedManifestDirectory, ()>,
    path: &[u8],
) -> TestShardedManifestEntry {
    match entry {
        Entry::Leaf(()) => TestShardedManifestEntry::File({
            TestShardedManifestFile {
                basename_length: path.len() as u64,
            }
        }),
        Entry::Tree(dir) => TestShardedManifestEntry::Directory(dir),
    }
}

async fn create_test_sharded_manifest_directory(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    subentries: TreeInfoSubentries<
        TestShardedManifestDirectory,
        (),
        (),
        LoadableShardedMapV2Node<TestShardedManifestEntry>,
    >,
) -> Result<TestShardedManifestDirectory> {
    let subentries: TrieMap<_> = subentries
        .into_iter()
        .map(|(prefix, entry_or_map)| match entry_or_map {
            Either::Left((_maybe_ctx, entry)) => {
                let entry = Either::Left(mf_entry_to_test_sharded_mf_entry(entry, prefix.as_ref()));
                (prefix, entry)
            }
            Either::Right(map) => (prefix, Either::Right(map)),
        })
        .collect();

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, subentries).await?;
    let max_basename_length = subentries.rollup_data();

    let mf = TestShardedManifest { subentries };
    let blob = mf.into_blob();
    let id = blob.store(&ctx, &blobstore).await?;

    Ok(TestShardedManifestDirectory {
        id,
        max_basename_length,
    })
}

async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<TestShardedManifestDirectory>,
    changes: Vec<(NonRootMPath, Option<()>)>,
    subtree_changes: Vec<ManifestParentReplacement<TestShardedManifestDirectory, ()>>,
) -> Result<Option<TestShardedManifestDirectory>> {
    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        subtree_changes,
        {
            cloned!(ctx, blobstore);
            move |info| {
                cloned!(ctx, blobstore);
                async move {
                    let directory =
                        create_test_sharded_manifest_directory(ctx, blobstore, info.subentries)
                            .await?;
                    Ok(((), directory))
                }
            }
        },
        move |_leaf_info| async move { Ok(((), ())) },
    )
    .await
}

pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootTestShardedManifestDirectory>,
    known: Option<&HashMap<ChangesetId, RootTestShardedManifestDirectory>>,
) -> Result<RootTestShardedManifestDirectory> {
    let parents = parents
        .into_iter()
        .map(|root| root.into_inner())
        .collect::<Vec<_>>();
    let changes = get_changes(&bonsai);
    let subtree_changes =
        get_test_sharded_manifest_subtree_changes(ctx, derivation_ctx, known, &bonsai).await?;
    let blobstore = derivation_ctx.blobstore();

    let root = inner_derive(ctx, blobstore, parents, changes, subtree_changes).await?;

    Ok(RootTestShardedManifestDirectory(match root {
        Some(root) => root,
        None => empty_directory(ctx, blobstore).await?,
    }))
}
