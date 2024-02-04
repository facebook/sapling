/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use manifest::derive_manifest;
use manifest::flatten_subentries;
use manifest::Entry;
use mononoke_types::test_manifest::TestManifest;
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::test_manifest::TestManifestEntry;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use skeleton_manifest::mapping::get_file_changes;

use crate::mapping::RootTestManifestDirectory;

async fn empty_directory(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<TestManifestDirectory> {
    let leaf = TestManifest::empty();
    let id = leaf.into_blob().store(ctx, blobstore).await?;

    Ok(TestManifestDirectory {
        id,
        max_basename_length: 0,
    })
}

fn get_changes(bcs: &BonsaiChangeset) -> Vec<(NonRootMPath, Option<()>)> {
    get_file_changes(bcs)
        .into_iter()
        .map(|(path, content)| (path, content.map(|_| ())))
        .collect()
}

async fn create_test_manifest_directory(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    subentries: impl Iterator<Item = (MPathElement, (Option<()>, Entry<TestManifestDirectory, ()>))>,
) -> Result<TestManifestDirectory> {
    let mut max_basename_length = 0;
    let subentries = subentries
        .inspect(|(path_element, (_ctx, entry))| match entry {
            Entry::Tree(dir) => {
                max_basename_length = std::cmp::max(max_basename_length, dir.max_basename_length);
            }
            Entry::Leaf(()) => {
                max_basename_length = std::cmp::max(max_basename_length, path_element.len() as u64);
            }
        })
        .map(|(path_element, (_ctx, entry))| match entry {
            Entry::Tree(dir) => (path_element, TestManifestEntry::Directory(dir)),
            Entry::Leaf(()) => (path_element, TestManifestEntry::File),
        });

    let mf = TestManifest::from_subentries(subentries);
    let blob = mf.into_blob();
    let id = blob
        .store(&ctx, &blobstore)
        .await
        .context("failed to store TestManifest blob")?;

    Ok(TestManifestDirectory {
        id,
        max_basename_length,
    })
}

async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<TestManifestDirectory>,
    changes: Vec<(NonRootMPath, Option<()>)>,
) -> Result<Option<TestManifestDirectory>> {
    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                cloned!(ctx, blobstore);
                async move {
                    let subentries = flatten_subentries(&ctx, &(), tree_info.subentries).await?;
                    let directory =
                        create_test_manifest_directory(ctx, blobstore, subentries).await?;

                    Ok(((), directory))
                }
            }
        },
        { move |_leaf_info| async move { Ok(((), ())) } },
    )
    .await
}

pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootTestManifestDirectory>,
) -> Result<RootTestManifestDirectory> {
    let parents = parents
        .into_iter()
        .map(|root| root.into_inner())
        .collect::<Vec<_>>();
    let changes = get_changes(&bonsai);
    let blobstore = derivation_ctx.blobstore();

    let root = inner_derive(ctx, blobstore, parents, changes).await?;

    Ok(RootTestManifestDirectory(match root {
        Some(root) => root,
        None => empty_directory(ctx, blobstore).await?,
    }))
}
