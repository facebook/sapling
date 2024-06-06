/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use blobstore::StoreLoadable;
use cloned::cloned;
use context::CoreContext;
use either::Either;
use filestore::FetchKey;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::derive_manifest_from_predecessor;
use manifest::Entry;
use manifest::ManifestComparison;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mercurial_types::ShardedHgAugmentedManifest;
use mononoke_types::hash::Blake3;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::MPathElement;
use mononoke_types::TrieMap;

/// Derive an HgAugmentedManifestId from an HgManifestId and parents.
pub async fn derive_from_hg_manifest_and_parents(
    ctx: &CoreContext,
    blobstore: &(impl Blobstore + 'static),
    hg_manifest_id: HgManifestId,
    parents: Vec<HgAugmentedManifestId>,
) -> Result<HgAugmentedManifestId> {
    let parents = parents.into_iter().map(Some).collect::<Vec<_>>();
    let (_, root, _, _) = bounded_traversal::bounded_traversal(
        256,
        (None, hg_manifest_id, parents),
        |(name, hg_manifest_id, parents): (Option<MPathElement>, _, _)| {
            async move {
                let (hg_manifest, parents) = future::try_join(
                    hg_manifest_id.load(ctx, blobstore),
                    future::try_join_all(parents.iter().map(|p| async move {
                        match p {
                            Some(p) => {
                                let aug_parent = p.load(ctx, blobstore).await?;
                                let hg_parent =
                                    HgManifestId::new(aug_parent.augmented_manifest.hg_node_id)
                                        .load(ctx, blobstore)
                                        .await?;
                                Ok((Some(aug_parent), Some(hg_parent)))
                            }
                            None => Ok((None, None)),
                        }
                    })),
                )
                .await?;

                let (aug_parents, hg_parents): (Vec<Option<_>>, Vec<Option<_>>) =
                    parents.into_iter().unzip();

                let mut children = Vec::new();
                let mut new_subentries = Vec::new();
                let mut reused_subentries = Vec::new();
                let mut reused_partial_maps = Vec::new();

                let mut diff =
                    manifest::compare_manifest(ctx, blobstore, hg_manifest.clone(), hg_parents)
                        .await?;

                while let Some(diff) = diff.try_next().await? {
                    match diff {
                        ManifestComparison::New(elem, entry) => match entry {
                            Entry::Tree(id) => {
                                children.push((Some(elem), id, Vec::new()));
                            }
                            Entry::Leaf((file_type, filenode)) => {
                                new_subentries.push((elem, file_type, filenode));
                            }
                        },
                        ManifestComparison::Changed(elem, entry, _parent_entries) => match entry {
                            Entry::Tree(id) => {
                                let child_parents = {
                                    let elem = &elem;
                                    let child_parents_futs: Vec<_> =
                                        aug_parents
                                            .iter()
                                            .map(|p| async move {
                                                match p {
                                                    Some(p) => match p
                                                        .augmented_manifest
                                                        .subentries
                                                        .lookup(ctx, blobstore, elem.as_ref())
                                                        .await?
                                                    {
                                                        Some(
                                                            HgAugmentedManifestEntry::DirectoryNode(
                                                                dir,
                                                            ),
                                                        ) => anyhow::Ok(Some(
                                                            HgAugmentedManifestId::new(
                                                                dir.treenode,
                                                            ),
                                                        )),
                                                        None
                                                        | Some(
                                                            HgAugmentedManifestEntry::FileNode(_),
                                                        ) => Ok(None),
                                                    },
                                                    None => Ok(None),
                                                }
                                            })
                                            .collect();
                                    future::try_join_all(child_parents_futs).await?
                                };
                                children.push((Some(elem), id, child_parents));
                            }
                            Entry::Leaf((file_type, filenode)) => {
                                new_subentries.push((elem, file_type, filenode));
                            }
                        },
                        ManifestComparison::Same(elem, _entry, _parent_entries, index) => {
                            let aug_parent = aug_parents
                                .get(index)
                                .and_then(Option::as_ref)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding parent {} of {}",
                                        index,
                                        hg_manifest_id
                                    )
                                })?;
                            let entry = aug_parent
                                .augmented_manifest
                                .subentries
                                .lookup(ctx, blobstore, elem.as_ref())
                                .await?
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding entry {} in parent {}",
                                        elem,
                                        aug_parent.augmented_manifest.hg_node_id,
                                    )
                                })?;
                            reused_subentries.push((elem, entry));
                        }
                        ManifestComparison::ManyNew(prefix, entries) => {
                            for (suffix, entry) in entries {
                                match entry {
                                    Entry::Tree(id) => {
                                        children.push((
                                            Some(prefix.clone().join_into_element(suffix)?),
                                            id,
                                            Vec::new(),
                                        ));
                                    }
                                    Entry::Leaf((file_type, filenode)) => {
                                        new_subentries.push((
                                            prefix.clone().join_into_element(suffix)?,
                                            file_type,
                                            filenode,
                                        ));
                                    }
                                }
                            }
                        }
                        ManifestComparison::ManySame(prefix, _entries, _parent_entries, index) => {
                            let aug_parent = aug_parents
                                .get(index)
                                .and_then(Option::as_ref)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding parent {} of {}",
                                        index,
                                        hg_manifest_id
                                    )
                                })?;
                            let partial_map = aug_parent
                                .augmented_manifest
                                .subentries
                                .get_partial_map(ctx, blobstore, prefix.as_ref())
                                .await?
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding prefix {:?} in parent {}",
                                        prefix,
                                        aug_parent.augmented_manifest.hg_node_id
                                    )
                                })?;
                            reused_partial_maps.push((prefix, partial_map));
                        }
                        ManifestComparison::Removed(..) | ManifestComparison::ManyRemoved(..) => {
                            // Removed items are omitted.
                        }
                    }
                }
                anyhow::Ok((
                    (
                        name,
                        hg_manifest,
                        new_subentries,
                        reused_subentries,
                        reused_partial_maps,
                    ),
                    children,
                ))
            }
            .boxed()
        },
        |(name, hg_manifest, new_subentries, reused_subentries, reused_partial_maps),
         children: bounded_traversal::Iter<(
            Option<MPathElement>,
            HgAugmentedManifestId,
            Blake3,
            u64,
        )>| {
            async move {
                let mut subentries = TrieMap::default();
                let new_subentries = stream::iter(new_subentries)
                    .map(|(elem, file_type, filenode_id)| {
                        anyhow::Ok(async move {
                            let filenode = filenode_id.load(ctx, blobstore).await?;
                            let metadata = filestore::get_metadata(
                                blobstore,
                                ctx,
                                &FetchKey::Canonical(filenode.content_id()),
                            )
                            .await?
                            .ok_or_else(|| anyhow!("Missing metadata for {}", filenode_id))?;
                            Ok((
                                elem,
                                HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode {
                                    file_type,
                                    filenode: filenode_id.into_nodehash(),
                                    total_size: metadata.total_size,
                                    content_blake3: metadata.seeded_blake3,
                                    content_sha1: metadata.sha1,
                                }),
                            ))
                        })
                    })
                    .try_buffered(100)
                    .try_collect::<Vec<_>>()
                    .await?;
                for (elem, entry) in new_subentries {
                    subentries.insert(elem, Either::Left(entry));
                }
                for (elem, entry) in reused_subentries {
                    subentries.insert(elem, Either::Left(entry));
                }
                for (name, treenode, augmented_manifest_id, augmented_manifest_size) in children {
                    subentries.insert(
                        name.expect("name must be defined for children"),
                        Either::Left(HgAugmentedManifestEntry::DirectoryNode(
                            HgAugmentedDirectoryNode {
                                treenode: treenode.into_nodehash(),
                                augmented_manifest_id,
                                augmented_manifest_size,
                            },
                        )),
                    );
                }
                for (prefix, partial_map) in reused_partial_maps {
                    subentries.insert(prefix, Either::Right(partial_map));
                }
                let subentries =
                    ShardedMapV2Node::from_entries_and_partial_maps(ctx, blobstore, subentries)
                        .await?;

                let hg_node_id = hg_manifest.node_id();
                let augmented_manifest = ShardedHgAugmentedManifest {
                    hg_node_id,
                    p1: hg_manifest.p1(),
                    p2: hg_manifest.p2(),
                    computed_node_id: hg_manifest.computed_node_id(),
                    subentries,
                };

                let (augmented_manifest_id, augmented_manifest_size) = augmented_manifest
                    .clone()
                    .compute_content_addressed_digest(ctx, blobstore)
                    .await?;

                let envelope = HgAugmentedManifestEnvelope {
                    augmented_manifest_id,
                    augmented_manifest_size,
                    augmented_manifest,
                };
                let hg_augmented_manifest_id = envelope.store(ctx, blobstore).await?;
                Ok((
                    name,
                    hg_augmented_manifest_id,
                    augmented_manifest_id,
                    augmented_manifest_size,
                ))
            }
            .boxed()
        },
    )
    .await?;
    Ok(root)
}

pub async fn derive_from_full_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    hg_manifest_id: HgManifestId,
) -> Result<HgAugmentedManifestId> {
    let root_entry = derive_manifest_from_predecessor(
        ctx.clone(),
        blobstore.clone(),
        hg_manifest_id,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                cloned!(ctx, blobstore);
                async move {
                    let hg_manifest = tree_info.predecessor.load(&ctx, &blobstore).await?;
                    let entries = tree_info.subentries.into_iter().map(|(elem, ((), entry))| {
                        let entry = match entry {
                            Entry::Tree(tree) => HgAugmentedManifestEntry::DirectoryNode(tree),
                            Entry::Leaf(leaf) => HgAugmentedManifestEntry::FileNode(leaf),
                        };
                        (elem, entry)
                    });
                    let subentries =
                        ShardedMapV2Node::from_entries(&ctx, &blobstore, entries).await?;
                    let augmented_manifest = ShardedHgAugmentedManifest {
                        hg_node_id: tree_info.predecessor.into_nodehash(),
                        p1: hg_manifest.p1(),
                        p2: hg_manifest.p2(),
                        computed_node_id: hg_manifest.computed_node_id(),
                        subentries,
                    };
                    let (augmented_manifest_id, augmented_manifest_size) = augmented_manifest
                        .clone()
                        .compute_content_addressed_digest(&ctx, &blobstore)
                        .await?;
                    let envelope = HgAugmentedManifestEnvelope {
                        augmented_manifest_id,
                        augmented_manifest_size,
                        augmented_manifest,
                    };
                    envelope.store(&ctx, &blobstore).await?;
                    let entry = HgAugmentedDirectoryNode {
                        treenode: tree_info.predecessor.into_nodehash(),
                        augmented_manifest_id,
                        augmented_manifest_size,
                    };
                    Ok(((), entry))
                }
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info| {
                cloned!(ctx, blobstore);
                async move {
                    let (file_type, filenode_id) = leaf_info.predecessor;
                    let filenode = filenode_id.load(&ctx, &blobstore).await?;
                    let metadata = filestore::get_metadata(
                        &blobstore,
                        &ctx,
                        &FetchKey::Canonical(filenode.content_id()),
                    )
                    .await?
                    .ok_or_else(|| anyhow!("Missing metadata for {}", filenode_id))?;
                    let hg_augmented_file_leaf_node = HgAugmentedFileLeafNode {
                        file_type,
                        filenode: filenode_id.into_nodehash(),
                        total_size: metadata.total_size,
                        content_blake3: metadata.seeded_blake3,
                        content_sha1: metadata.sha1,
                    };
                    Ok(((), hg_augmented_file_leaf_node))
                }
            }
        },
    )
    .await?;
    Ok(HgAugmentedManifestId::new(root_entry.treenode))
}
