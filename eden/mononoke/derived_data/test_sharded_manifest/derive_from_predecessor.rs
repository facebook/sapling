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
use itertools::Either;
use manifest::derive_manifest_from_predecessor;
use manifest::Entry;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifest;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::test_sharded_manifest::TestShardedManifestFile;
use mononoke_types::BlobstoreValue;
use mononoke_types::MPathElement;
use mononoke_types::TrieMap;
use sorted_vector_map::SortedVectorMap;
use test_manifest::RootTestManifestDirectory;

use crate::RootTestShardedManifestDirectory;

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
    subentries: SortedVectorMap<MPathElement, ((), Entry<TestShardedManifestDirectory, ()>)>,
) -> Result<TestShardedManifestDirectory> {
    let subentries_map = subentries
        .into_iter()
        .map(|(path, (_ctx, entry))| {
            let entry = Either::Left(mf_entry_to_test_sharded_mf_entry(entry, path.as_ref()));
            (path, entry)
        })
        .collect::<TrieMap<_>>();

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, subentries_map).await?;
    let max_basename_length = subentries.rollup_data();

    let mf = TestShardedManifest { subentries };
    let blob = mf.into_blob();
    let id = blob
        .store(&ctx, &blobstore)
        .await
        .context("failed to store TestShardedManifest blob")?;

    Ok(TestShardedManifestDirectory {
        id,
        max_basename_length,
    })
}

pub(crate) async fn inner_derive_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    predecessor: TestManifestDirectory,
) -> Result<TestShardedManifestDirectory> {
    derive_manifest_from_predecessor(
        ctx.clone(),
        blobstore.clone(),
        predecessor,
        {
            cloned!(ctx, blobstore);
            move |info| {
                cloned!(ctx, blobstore);
                async move {
                    let directory =
                        create_test_sharded_manifest_directory(ctx, blobstore, info.subentries)
                            .await?;
                    anyhow::Ok(((), directory))
                }
            }
        },
        { move |_leaf_info| async move { Ok(((), ())) } },
    )
    .await
}

pub(crate) async fn derive_from_predecessor(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    predecessor: RootTestManifestDirectory,
) -> Result<RootTestShardedManifestDirectory> {
    let predecessor = predecessor.into_inner();
    let blobstore = derivation_ctx.blobstore();

    Ok(RootTestShardedManifestDirectory(
        inner_derive_from_predecessor(ctx, blobstore, predecessor).await?,
    ))
}
