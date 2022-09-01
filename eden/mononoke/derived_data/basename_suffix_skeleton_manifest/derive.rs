/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! See docs/basename_suffix_skeleton_manifest.md for more information

use std::sync::Arc;

use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future::FutureExt;
use manifest::derive_manifest_with_io_sender;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::TreeInfo;
use mononoke_types::basename_suffix_skeleton_manifest::BasenameSuffixSkeletonManifest;
use mononoke_types::basename_suffix_skeleton_manifest::BssmDirectory;
use mononoke_types::basename_suffix_skeleton_manifest::BssmEntry;
use mononoke_types::BasenameSuffixSkeletonManifestId;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use skeleton_manifest::mapping::get_file_changes;

use crate::mapping::RootBasenameSuffixSkeletonManifest;
use crate::path::BsmPath;

/// Calculate a list of changes of the changeset, but putting the basename first and
/// reversing it.
fn get_fixed_up_changes(bcs: &BonsaiChangeset) -> Vec<(MPath, Option<(ContentId, FileType)>)> {
    get_file_changes(bcs)
        .into_iter()
        .map(|(path, content)| (BsmPath::transform(path).into_raw(), content))
        .collect()
}

async fn empty_mf(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
) -> Result<BasenameSuffixSkeletonManifestId> {
    let leaf = BasenameSuffixSkeletonManifest::empty();
    leaf.into_blob().store(ctx, blobstore).await
}

async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<BssmDirectory>,
    changes: Vec<(MPath, Option<(ContentId, FileType)>)>,
) -> Result<Option<BssmDirectory>> {
    // Types to help understand how to use derive_manifest helper
    type Leaf = (ContentId, FileType);
    type LeafId = ();
    type TreeId = BssmDirectory;
    type IntermediateLeafId = LeafId;
    type Ctx = ();
    derive_manifest_with_io_sender(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        // create_tree
        {
            cloned!(ctx, blobstore);
            move |info: TreeInfo<TreeId, IntermediateLeafId, Ctx>, fut_sender| {
                cloned!(ctx, blobstore);
                async move {
                    let entries =
                        info.subentries
                            .into_iter()
                            .map(|(path_el, (_maybe_ctx, entry_in))| {
                                let entry = match entry_in {
                                    Entry::Leaf(()) => BssmEntry::File,
                                    Entry::Tree(entry) => BssmEntry::Directory(entry),
                                };
                                (path_el, Some(entry))
                            });

                    let (mf, rollup_count) = BasenameSuffixSkeletonManifest::empty()
                        .update(&ctx, &blobstore, entries.collect())
                        .await?;
                    let entry = {
                        let blob = mf.into_blob();
                        let id = *blob.id();
                        fut_sender
                            .unbounded_send(
                                async move { blob.store(&ctx, &blobstore).await.map(|_| ()) }
                                    .boxed(),
                            )
                            .map_err(|err| {
                                anyhow::anyhow!("failed to send manifest future {}", err)
                            })?;
                        BssmDirectory {
                            id,
                            rollup_count: (1 + rollup_count) as u64,
                        }
                    };
                    anyhow::Ok(((), entry))
                }
            }
        },
        // create_leaf
        {
            move |_leaf_info: LeafInfo<IntermediateLeafId, Leaf>, _fut_sender| async move {
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
    parents: Vec<RootBasenameSuffixSkeletonManifest>,
) -> Result<RootBasenameSuffixSkeletonManifest> {
    let parents = parents.into_iter().map(|root| root.0).collect::<Vec<_>>();
    let changes = get_fixed_up_changes(&bonsai);
    let blobstore = derivation_ctx.blobstore();
    // TODO(yancouto): Optimise by doing the first query separately using the optimisations
    // in sharded map, which are unused in common manifest code recently
    let root = inner_derive(ctx, blobstore, parents, changes).await?;
    Ok(RootBasenameSuffixSkeletonManifest(match root {
        Some(root) => root,
        // Only happens on empty repo
        None => BssmDirectory {
            id: empty_mf(ctx, blobstore).await?,
            rollup_count: 1,
        },
    }))
}
