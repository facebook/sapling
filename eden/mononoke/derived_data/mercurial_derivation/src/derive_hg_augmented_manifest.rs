/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
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
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::ManifestComparison;
use manifest::derive_manifest_from_predecessor;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mercurial_types::ShardedHgAugmentedManifest;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::MPathElementPrefix;
use mononoke_types::RepoPath;
use mononoke_types::TrieMap;
use mononoke_types::hash::Blake3;
use mononoke_types::sharded_map_v2::LookupKind;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedPathManifestIdEntry;
use restricted_paths::RestrictedPaths;

/// Derive an HgAugmentedManifestId from an HgManifestId and parents.
pub async fn derive_from_hg_manifest_and_parents(
    ctx: &CoreContext,
    blobstore: &(impl Blobstore + 'static),
    hg_manifest_id: HgManifestId,
    parents: Vec<HgAugmentedManifestId>,
    content_metadata_cache: &HashMap<ContentId, ContentMetadataV2>,
    restricted_paths: &RestrictedPaths,
) -> Result<HgAugmentedManifestId> {
    let restricted_paths_enabled = justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None, // hashing
        // Adding a switch value to be able to disable writes only
        Some("hg_augmented_manifest_write"),
    )?;
    let parents = parents.into_iter().map(Some).collect::<Vec<_>>();
    let (_, root, _, _) = bounded_traversal::bounded_traversal(
        256,
        (MPath::ROOT, None, hg_manifest_id, parents),
        |(parent_path, name, hg_manifest_id, parents): (MPath, Option<MPathElement>, _, _)| {
            async move {
                let path = parent_path.join_element(name.as_ref());
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

                // For each parent we store a list consisting of path elements to fetch entries for,
                // and path element prefixes to fetch partial maps for.
                let mut path_elems_to_fetch_from_aug_parents: Vec<
                    Vec<Either<MPathElement, MPathElementPrefix>>,
                > = vec![vec![]; aug_parents.len()];

                let mut diff =
                    manifest::compare_manifest(ctx, blobstore, hg_manifest.clone(), hg_parents)
                        .await?;

                while let Some(diff) = diff.try_next().await? {
                    match diff {
                        ManifestComparison::New(elem, entry) => match entry {
                            Entry::Tree(id) => {
                                children.push((path.clone(), Some(elem), id, Vec::new()));
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
                                children.push((path.clone(), Some(elem), id, child_parents));
                            }
                            Entry::Leaf((file_type, filenode)) => {
                                new_subentries.push((elem, file_type, filenode));
                            }
                        },
                        ManifestComparison::Same(elem, _entry, _parent_entries, index) => {
                            path_elems_to_fetch_from_aug_parents
                                .get_mut(index)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding parent {} of {} (ManifestComparison::Same)",
                                        index,
                                        hg_manifest_id
                                    )
                                })?
                                .push(Either::Left(elem));
                        }
                        ManifestComparison::ManyNew(prefix, entries) => {
                            for (suffix, entry) in entries {
                                match entry {
                                    Entry::Tree(id) => {
                                        children.push((
                                            path.clone(),
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
                            path_elems_to_fetch_from_aug_parents
                                .get_mut(index)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding parent {} of {} (ManifestComparison::ManySame)",
                                        index,
                                        hg_manifest_id
                                    )
                                })?
                                .push(Either::Right(prefix));
                        }
                        ManifestComparison::Removed(..) | ManifestComparison::ManyRemoved(..) => {
                            // Removed items are omitted.
                        }
                    }
                }

                let reused_subentries_and_partial_maps =
                    path_elems_to_fetch_from_aug_parents.into_iter().enumerate().map(
                        |(index, path_elems_or_prefixes)| {
                            let aug_parents = &aug_parents;
                            async move {
                                if path_elems_or_prefixes.is_empty() {
                                    return anyhow::Ok(futures::stream::empty().left_stream());
                                }

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

                                let lookup_keys = path_elems_or_prefixes.into_iter().map(|path_elem_or_prefix| {
                                    match path_elem_or_prefix {
                                        Either::Left(elem) => (elem.to_smallvec(), LookupKind::Entry),
                                        Either::Right(prefix) => (prefix.to_smallvec(), LookupKind::PartialMap),
                                    }
                                })
                                .collect();

                                Ok(
                                    stream::iter(
                                        aug_parent
                                            .augmented_manifest
                                            .subentries
                                            .clone()
                                            .get_entries_and_partial_maps(ctx, blobstore, lookup_keys, 100).await?
                                    )
                                    .map(anyhow::Ok)
                                    .right_stream()
                                )
                            }
                        },
                    )
                    .collect::<FuturesUnordered<_>>()
                    .try_flatten()
                    .try_collect::<Vec<_>>()
                    .await?;

                anyhow::Ok((
                    (
                        path,
                        name,
                        hg_manifest,
                        new_subentries,
                        reused_subentries_and_partial_maps,
                    ),
                    children,
                ))
            }
            .boxed()
        },
        |(path, name, hg_manifest, new_subentries, reused_subentries_and_partial_maps),
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
                            let content_id = filenode.content_id();
                            let metadata =
                                get_metadata(ctx, blobstore, content_metadata_cache, content_id)
                                    .await?;
                            Ok((
                                elem,
                                HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode {
                                    file_type,
                                    filenode: filenode_id.into_nodehash(),
                                    total_size: metadata.total_size,
                                    content_blake3: metadata.seeded_blake3,
                                    content_sha1: metadata.sha1,
                                    file_header_metadata: if filenode.metadata().is_empty() {
                                        None
                                    } else {
                                        Some(filenode.metadata().clone())
                                    },
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
                for (key, reused_item) in reused_subentries_and_partial_maps {
                    subentries.insert(key, reused_item);
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

                if restricted_paths_enabled
                    && let Some(non_root_path) = path.clone().into_optional_non_root_path()
                    && restricted_paths.is_restricted_path(&non_root_path)
                {
                    let entry = RestrictedPathManifestIdEntry::new(
                        ManifestType::HgAugmented,
                        hg_augmented_manifest_id.to_string().into(),
                        RepoPath::DirectoryPath(non_root_path),
                    )?;

                    if let Err(e) = restricted_paths
                        .manifest_id_store()
                        .add_entry(ctx, entry)
                        .await
                    {
                        // Log error but don't fail manifest derivation
                        slog::warn!(ctx.logger(), "Failed to track restricted path at {path}: {e}");
                    }
                }
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

async fn get_metadata<'a>(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    content_metadata_cache: &'a HashMap<ContentId, ContentMetadataV2>,
    content_id: ContentId,
) -> Result<Cow<'a, ContentMetadataV2>> {
    Ok(match content_metadata_cache.get(&content_id) {
        Some(metadata) => Cow::Borrowed(metadata),
        None => {
            let metadata =
                filestore::get_metadata(blobstore, ctx, &FetchKey::Canonical(content_id))
                    .await?
                    .ok_or_else(|| anyhow!("Missing metadata for {}", content_id))?;
            Cow::Owned(metadata)
        }
    })
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
                        file_header_metadata: if filenode.metadata().is_empty() {
                            None
                        } else {
                            Some(filenode.metadata().clone())
                        },
                    };
                    Ok(((), hg_augmented_file_leaf_node))
                }
            }
        },
    )
    .await?;
    Ok(HgAugmentedManifestId::new(root_entry.treenode))
}
