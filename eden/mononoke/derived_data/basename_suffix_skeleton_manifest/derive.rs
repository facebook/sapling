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
use blobstore::Loadable;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future::FutureExt;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
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
use mononoke_types::MPathElement;
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

fn split_by_root_dir<X>(changes: Vec<(MPath, X)>) -> HashMap<MPathElement, Vec<(MPath, X)>> {
    let mut map = HashMap::new();
    for (p, x) in changes {
        let (root_dir, rest) = p.split_first();
        let rest = rest.expect("We always add a sentinel suffix to the path");
        map.entry(root_dir.clone())
            .or_insert_with(Vec::new)
            .push((rest, x));
    }
    map
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
    const CONCURRENCY: usize = 100;
    let parents = parents.into_iter().map(|root| root.0).collect::<Vec<_>>();
    let changes = get_fixed_up_changes(&bonsai);
    let blobstore = derivation_ctx.blobstore();
    // TODO(T123518092): Once T123518092 is done, this optimisation can be removed and
    // we can call `inner_derive` as in the `else` clause.
    let root = if parents.len() <= 1 {
        let (parent, previous_rollup) = match parents.into_iter().next() {
            Some(p) => (Some(p.load(ctx, blobstore).await?), p.rollup_count),
            None => (None, 1),
        };
        let entries_to_update = stream::iter(split_by_root_dir(changes))
            .map(|(root_dir, changes)| async {
                let new_parent = match parent.as_ref() {
                    None => vec![],
                    Some(p) => match p.lookup(ctx, blobstore, &root_dir).await? {
                        Some(BssmEntry::Directory(dir)) => vec![dir],
                        None => vec![],
                        other => anyhow::bail!("Invalid directory {:?}", other),
                    },
                };
                Ok((
                    root_dir,
                    inner_derive(ctx, blobstore, new_parent, changes)
                        .await?
                        .map(BssmEntry::Directory),
                ))
            })
            .buffer_unordered(CONCURRENCY)
            .try_collect()
            .await?;
        let (mf, rollup_diff) = parent
            .unwrap_or_else(BasenameSuffixSkeletonManifest::empty)
            .update(ctx, blobstore, entries_to_update)
            .await?;
        Some(BssmDirectory {
            id: mf.into_blob().store(ctx, blobstore).await?,
            rollup_count: ((previous_rollup as i64) + rollup_diff) as u64,
        })
    } else {
        inner_derive(ctx, blobstore, parents, changes).await?
    };
    Ok(RootBasenameSuffixSkeletonManifest(match root {
        Some(root) => root,
        // Only happens on empty repo
        None => BssmDirectory {
            id: empty_mf(ctx, blobstore).await?,
            rollup_count: 1,
        },
    }))
}
