/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blobstore::KeyedBlobstore;
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
use manifest::LeafInfo;
use manifest::ManifestComparison;
use manifest::ManifestOps;
use manifest::Span;
use manifest::Traced;
use manifest::TreeInfo;
use manifest::TreeInfoSubentries;
use manifest::derive_manifest;
use manifest::derive_manifest_from_predecessor;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::ShardedHgAugmentedManifest;
use mercurial_types::blobs::ContentBlobMeta;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::calculate_hg_node_id_stream;
use mercurial_types::calculate_hg_node_ids_multi;
use mercurial_types::preloaded_augmented_manifest::serialize_manifest_entry;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::MPathElementPrefix;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use mononoke_types::SortedVectorTrieMap;
use mononoke_types::TrackedFileChange;
use mononoke_types::TrieMap;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::hash::Blake3;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::LookupKind;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestId;
use restricted_paths_common::ArcRestrictedPathsConfigBased;
use restricted_paths_common::ManifestType;
use restricted_paths_common::RestrictedPathManifestIdEntry;
use restricted_paths_common::RestrictedPathsConfigBased;
use tracing::warn;

use crate::acl_overlay_manifest::AclOverlayHgManifestId;
use crate::derive_hg_manifest::ParentIndex;
use crate::derive_hg_manifest::hg_parents;

/// Derive an HgAugmentedManifestId from an HgManifestId and parents.
///
/// Canonical wrapper around [`derive_from_hg_manifest_and_parents_staged`]:
/// derives the whole tree starting at the root (always a tree) with no known
/// entries. With `stage_path == MPath::ROOT` and empty `known_entries`, the
/// staged core is byte-identical to the original hand-rolled traversal.
pub async fn derive_from_hg_manifest_and_parents(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    hg_manifest_id: HgManifestId,
    parents: Vec<HgAugmentedManifestId>,
    content_metadata_cache: &HashMap<ContentId, ContentMetadataV2>,
    restricted_paths: &RestrictedPathsConfigBased,
    acl_root_overlay: Option<AclManifestId>,
) -> Result<HgAugmentedManifestId> {
    let entry = derive_from_hg_manifest_and_parents_staged(
        ctx,
        blobstore,
        MPath::ROOT,
        Entry::Tree(hg_manifest_id),
        parents.into_iter().map(Some).collect(),
        HashMap::new(),
        content_metadata_cache,
        restricted_paths,
        acl_root_overlay,
    )
    .await?;
    match entry {
        Some(HgAugmentedManifestEntry::DirectoryNode(dir)) => {
            Ok(HgAugmentedManifestId::new(dir.treenode))
        }
        _ => bail!("root augmented manifest entry must be a directory"),
    }
}

/// Stage-scoped core of augmented manifest derivation.
///
/// Derives the augmented manifest subtree rooted at `stage_path`, returning the
/// entry at that path (a `DirectoryNode` for trees, a `FileNode` when the stage
/// root is a file, or `None` when nothing exists at the stage path).
///
/// `parents` are the parent augmented subtrees at `stage_path` (positional,
/// bonsai-parent order; `None` for parents with nothing there). `known_entries`
/// holds already-derived child-stage outputs keyed by absolute path; when the
/// traversal reaches one of those paths it reuses the entry instead of
/// recursing -- the same short-circuit the manifest crate's
/// `derive_manifest_with_known_entries` performs. This is byte-identical because
/// each known entry is content-addressed.
///
/// `hg_entry` is the stage-root entry from the hg manifest: a tree to traverse,
/// or a leaf when the stage path resolves to a file in this commit.
pub async fn derive_from_hg_manifest_and_parents_staged(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    stage_path: MPath,
    hg_entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
    parents: Vec<Option<HgAugmentedManifestId>>,
    known_entries: HashMap<MPath, Option<HgAugmentedManifestEntry>>,
    content_metadata_cache: &HashMap<ContentId, ContentMetadataV2>,
    restricted_paths: &RestrictedPathsConfigBased,
    acl_overlay: Option<AclManifestId>,
) -> Result<Option<HgAugmentedManifestEntry>> {
    // The stage root may itself be a file (a configured stage path that resolves
    // to a file in this commit). There is no tree to traverse, so build the
    // single augmented leaf and return it.
    let hg_manifest_id = match hg_entry {
        Entry::Leaf((file_type, filenode_id)) => {
            let leaf = build_augmented_file_leaf(
                ctx,
                blobstore,
                content_metadata_cache,
                file_type,
                filenode_id,
            )
            .await?;
            return Ok(Some(HgAugmentedManifestEntry::FileNode(leaf)));
        }
        Entry::Tree(hg_manifest_id) => hg_manifest_id,
    };

    let restricted_paths_enabled = justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None, // hashing
        // Adding a switch value to be able to disable writes only
        Some("hg_augmented_manifest_write"),
    );
    let stage_path = &stage_path;
    // Already-derived child-stage subtrees, keyed by absolute path. When the
    // traversal reaches one of these paths it reuses the entry instead of
    // recursing, at any depth.
    let known_entries = &known_entries;

    let root = bounded_traversal::bounded_traversal(
        256,
        (stage_path.clone(), None, hg_manifest_id, parents, acl_overlay),
        |(parent_path, name, hg_manifest_id, parents, acl_overlay): (MPath, Option<MPathElement>, _, _, _)| {
            async move {
                // At the traversal root, `name` is None and this is the stage
                // path itself (`join_element(None)` returns the parent path).
                let path = parent_path.join_element(name.as_ref());
                // Short-circuit a subtree already derived in an earlier stage:
                // emit it directly without loading, diffing, or recursing. Only
                // directories are reached here -- files are handled as leaves by
                // the parent and rebuilt identically, and a `None` known entry is
                // never reached because that path is absent from the hg manifest.
                if let Some(Some(HgAugmentedManifestEntry::DirectoryNode(dir))) =
                    known_entries.get(&path)
                {
                    let children: Vec<(
                        MPath,
                        Option<MPathElement>,
                        HgManifestId,
                        Vec<Option<HgAugmentedManifestId>>,
                        Option<AclManifestId>,
                    )> = Vec::new();
                    return anyhow::Ok((Either::Left((name, dir.clone())), children));
                }
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

                // Build ACL child directory map from the current overlay
                let child_acl_map = match &acl_overlay {
                    Some(acl_id) => load_acl_child_directory_map(ctx, blobstore, acl_id).await?,
                    None => HashMap::new(),
                };

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
                        ManifestComparison::New(span) => {
                            let mut handle_new = |elem: MPathElement, entry| match entry {
                                Entry::Tree(id) => {
                                    let child_acl = child_acl_map.get(&elem).copied();
                                    children.push((
                                        path.clone(),
                                        Some(elem),
                                        id,
                                        Vec::new(),
                                        child_acl,
                                    ));
                                }
                                Entry::Leaf((file_type, filenode)) => {
                                    new_subentries.push((elem, file_type, filenode));
                                }
                            };
                            match span {
                                Span::Element(elem, entry) => handle_new(elem, entry),
                                Span::Prefix(prefix, entries) => {
                                    for (suffix, entry) in entries {
                                        let elem = prefix.clone().join_into_element(suffix)?;
                                        handle_new(elem, entry);
                                    }
                                }
                            }
                        }
                        ManifestComparison::Changed(elem, entry, _parent_entries) => {
                            match entry {
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
                                let child_acl = child_acl_map.get(&elem).copied();
                                children.push((path.clone(), Some(elem), id, child_parents, child_acl));
                            }
                            Entry::Leaf((file_type, filenode)) => {
                                new_subentries.push((elem, file_type, filenode));
                            }
                            }
                        }
                        ManifestComparison::Same(span, index) => {
                            let key = match span {
                                Span::Element(elem, _entry) => Either::Left(elem),
                                Span::Prefix(prefix, _entries) => Either::Right(prefix),
                            };
                            path_elems_to_fetch_from_aug_parents
                                .get_mut(index)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "Cannot find corresponding parent {index} of {hg_manifest_id} (ManifestComparison::Same)"
                                    )
                                })?
                                .push(key);
                        }
                        ManifestComparison::Removed(_) => {
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
                                            "Cannot find corresponding parent {index} of {hg_manifest_id}"
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
                    Either::Right((
                        path,
                        name,
                        hg_manifest,
                        new_subentries,
                        reused_subentries_and_partial_maps,
                        child_acl_map,
                        acl_overlay,
                    )),
                    children,
                ))
            }
            .boxed()
        },
        |out,
         children: bounded_traversal::Iter<(
            Option<MPathElement>,
            HgAugmentedManifestId,
            Blake3,
            u64,
        )>| {
            async move {
                // A short-circuited known subtree: emit it directly. The parent
                // applies its own child_acl_map to the directory pointer, so this
                // subtree's own acl is not needed here.
                let (path, name, hg_manifest, new_subentries, reused_subentries_and_partial_maps, child_acl_map, acl_overlay) =
                    match out {
                        Either::Left((name, dir)) => {
                            return Ok((
                                name,
                                HgAugmentedManifestId::new(dir.treenode),
                                dir.augmented_manifest_id,
                                dir.augmented_manifest_size,
                            ));
                        }
                        Either::Right(state) => state,
                    };
                let mut subentries = TrieMap::default();
                let new_subentries = stream::iter(new_subentries)
                    .map(|(elem, file_type, filenode_id)| {
                        anyhow::Ok(async move {
                            let leaf = build_augmented_file_leaf(
                                ctx,
                                blobstore,
                                content_metadata_cache,
                                file_type,
                                filenode_id,
                            )
                            .await?;
                            Ok((elem, HgAugmentedManifestEntry::FileNode(leaf)))
                        })
                    })
                    .try_buffered(100)
                    .try_collect::<Vec<_>>()
                    .await?;
                for (elem, entry) in new_subentries {
                    subentries.insert(elem, Either::Left(entry));
                }
                // Reused entries from parents are safe to use with their existing ACL
                // pointers: ACL pointer changes only occur when .slacl files are
                // added/removed, which always causes ManifestComparison::Changed on
                // the path to the .slacl file. Directories not on such a path either
                // (a) are not in the ACL tree (pointer is None) or (b) have an
                // unchanged ACL subtree (pointer remains valid).
                for (key, reused_item) in reused_subentries_and_partial_maps {
                    subentries.insert(key, reused_item);
                }
                for (name, treenode, augmented_manifest_id, augmented_manifest_size) in children {
                    let child_name = name.ok_or_else(|| {
                        anyhow!("child name must be defined for non-root traversal nodes")
                    })?;
                    let child_acl_id = child_acl_map.get(&child_name).copied();
                    subentries.insert(
                        child_name,
                        Either::Left(HgAugmentedManifestEntry::DirectoryNode(
                            HgAugmentedDirectoryNode {
                                treenode: treenode.into_nodehash(),
                                augmented_manifest_id,
                                augmented_manifest_size,
                                acl_manifest_directory_id: child_acl_id,
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
                    acl_manifest_directory_id: acl_overlay,
                };

                // Enforce root invariant: the repo-root pointer must never be
                // Some(canonical_empty_acl_manifest_id). Only the true repo root
                // (stage root at MPath::ROOT) is subject to this; a non-root
                // stage root is an interior directory.
                if name.is_none() && stage_path.is_root() {
                    assert_root_acl_pointer_invariant(
                        &augmented_manifest.acl_manifest_directory_id,
                    )?;
                }

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

                if restricted_paths_enabled {
                    if let Some(non_root_path) = path.clone().into_optional_non_root_path() {
                        let should_record = restricted_paths
                            .should_record_manifest_id_entry(&non_root_path);
                        if should_record {
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
                                warn!("Failed to track restricted path at {path}: {e}");
                            }
                        }
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
    let (_, hg_augmented_manifest_id, augmented_manifest_id, augmented_manifest_size) = root;
    Ok(Some(HgAugmentedManifestEntry::DirectoryNode(
        HgAugmentedDirectoryNode {
            treenode: hg_augmented_manifest_id.into_nodehash(),
            augmented_manifest_id,
            augmented_manifest_size,
            acl_manifest_directory_id: acl_overlay,
        },
    )))
}

/// Build the augmented file leaf for a single filenode, attaching content
/// blake3, sha1, and size from metadata (the prefetched cache, falling back to
/// the filestore).
async fn build_augmented_file_leaf(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    content_metadata_cache: &HashMap<ContentId, ContentMetadataV2>,
    file_type: FileType,
    filenode_id: HgFileNodeId,
) -> Result<HgAugmentedFileLeafNode> {
    let filenode = filenode_id.load(ctx, blobstore).await?;
    let metadata = get_metadata(
        ctx,
        blobstore,
        content_metadata_cache,
        filenode.content_id(),
    )
    .await?;
    Ok(HgAugmentedFileLeafNode {
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
    })
}

async fn get_metadata<'a>(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    content_metadata_cache: &'a HashMap<ContentId, ContentMetadataV2>,
    content_id: ContentId,
) -> Result<Cow<'a, ContentMetadataV2>> {
    Ok(match content_metadata_cache.get(&content_id) {
        Some(metadata) => Cow::Borrowed(metadata),
        None => {
            let metadata =
                filestore::get_metadata(blobstore, ctx, &FetchKey::Canonical(content_id))
                    .await?
                    .ok_or_else(|| anyhow!("Missing metadata for {content_id}"))?;
            Cow::Owned(metadata)
        }
    })
}

/// Normalize a derived ACL root into an optional overlay.
/// Returns `None` if the JK is disabled or the ACL manifest is the canonical
/// empty one (no .slacl files), `Some(id)` otherwise.
pub fn normalize_acl_root(
    root_acl_manifest_id: &acl_manifest::RootAclManifestId,
) -> Result<Option<AclManifestId>> {
    let add_acl_manifest_pointer =
        justknobs::eval("scm/mononoke:add_acl_manifest_pointer", None, None);
    if !add_acl_manifest_pointer {
        return Ok(None);
    }
    let id = *root_acl_manifest_id.inner_id();
    if id == AclManifest::empty_id() {
        Ok(None)
    } else {
        Ok(Some(id))
    }
}

/// Pre-resolve copy-from source filenodes from parent augmented manifests.
///
/// For each file change with copy_from metadata, looks up the source path
/// in the appropriate parent augmented manifest to get the filenode hash.
/// Returns a map from (copy_path, copy_csid) to the resolved HgFileNodeId.
pub async fn resolve_copy_from_filenodes<Store>(
    ctx: &CoreContext,
    blobstore: &Store,
    file_changes: &[(NonRootMPath, Option<TrackedFileChange>)],
    parents: &[Option<(ChangesetId, HgAugmentedManifestId)>; 2],
) -> Result<HashMap<(NonRootMPath, ChangesetId), HgFileNodeId>>
where
    Store: KeyedBlobstore + Clone + 'static,
{
    // Group copy-from paths by parent index. Mercurial filenodes only encode
    // (p1, p2); copy-from sources pointing at step-parents in octopus merges
    // are skipped to match the existing HgManifest-based path. The originating
    // ChangesetId is implicit in the slot (== parents[idx].0).
    let mut paths_by_parent: [Vec<NonRootMPath>; 2] = [Vec::new(), Vec::new()];

    for (_, change) in file_changes {
        let Some(change) = change.as_ref() else {
            continue;
        };
        let Some((copy_path, copy_csid)) = change.copy_from() else {
            continue;
        };
        if let Some(idx) = parents
            .iter()
            .position(|p| p.as_ref().is_some_and(|(c, _)| c == copy_csid))
        {
            paths_by_parent[idx].push(copy_path.clone());
        }
    }

    // `.copied()` yields owned `Option<(ChangesetId, HgAugmentedManifestId)>`
    // (both are `Copy`) so the async closure doesn't borrow across the await
    // boundary — required for the `derive_single`/`derive_batch` trait bounds
    // to accept it.
    stream::iter(paths_by_parent.into_iter().zip(parents.iter().copied()))
        .map(|(paths, parent)| async move {
            let Some((cs_id, parent_aug_manifest_id)) = parent else {
                return Ok::<HashMap<_, _>, anyhow::Error>(HashMap::new());
            };
            if paths.is_empty() {
                return Ok(HashMap::new());
            }
            parent_aug_manifest_id
                .find_entries(
                    ctx.clone(),
                    blobstore.clone(),
                    paths
                        .into_iter()
                        .map(|p| manifest::PathOrPrefix::Path(p.into())),
                )
                .try_filter_map(|(path, entry)| async move {
                    match entry {
                        Entry::Leaf(leaf) => {
                            let non_root = NonRootMPath::try_from(path)
                                .map_err(|_| anyhow!("Expected non-root path in manifest"))?;
                            Ok(Some(((non_root, cs_id), HgFileNodeId::new(leaf.filenode))))
                        }
                        Entry::Tree(_) => Ok(None),
                    }
                })
                .try_collect::<HashMap<_, _>>()
                .await
        })
        .buffer_unordered(2)
        .try_concat()
        .await
}

/// Decision for a leaves-only merge conflict.
#[derive(Debug)]
pub(crate) enum LeafConflictResolution {
    /// Reuse an hg-relevant parent's leaf (p1/p2).
    Reuse(HgAugmentedFileLeafNode),
    /// No hg-relevant parent has the file; caller must mint a parentless
    /// filenode using this representative content.
    MintFresh(HgAugmentedFileLeafNode),
}

/// Resolve a leaves-only conflict to match the HgManifest path.
///
/// All contributing parents must agree on file type and content. Only p1/p2 can
/// provide reusable Mercurial filenode parentage; step-parent-only files require
/// minting a parentless filenode.
pub(crate) fn check_content_identical_at_parents(
    path: &NonRootMPath,
    parents: &[Traced<ParentIndex, HgAugmentedFileLeafNode>],
) -> Result<LeafConflictResolution> {
    use crate::derive_hg_manifest::unique_or_nothing;

    let Some(representative) = parents.first().map(|t| t.untraced()) else {
        bail!("Unresolved leaf conflict at {path}: no contributing parents");
    };

    unique_or_nothing(parents.iter().map(|t| t.untraced().file_type))
        .with_context(|| format!("Unresolved file type conflict at {path}"))?;

    // Match the legacy HgManifest resolver's content identity check.
    unique_or_nothing(
        parents
            .iter()
            .map(|t| (t.untraced().content_sha1, t.untraced().total_size)),
    )
    .with_context(|| format!("Unresolved content conflict at {path}"))?;

    // Step-parents affect conflict detection but cannot be filenode parents.
    let (maybe_reuse_leaf, _) = hg_parents(parents);
    match maybe_reuse_leaf {
        Some(leaf) => Ok(LeafConflictResolution::Reuse(leaf)),
        None => {
            // Content matches, but no reusable hg parent exists.
            Ok(LeafConflictResolution::MintFresh(representative.clone()))
        }
    }
}

/// Mint a fresh parentless Mercurial filenode for a file that appears only in
/// the step-parents (p3+) of an octopus merge, and build the augmented leaf for
/// it. Mirrors `resolve_conflict`'s no-reusable-filenode branch in
/// `derive_hg_manifest.rs`: a filenode with `p1 = p2 = None` and no copy
/// metadata is uploaded for the (already content-identical) data, so both the
/// direct and reference paths produce the same filenode and thus the same
/// augmented manifest.
async fn mint_parentless_leaf(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    path: &NonRootMPath,
    src: &HgAugmentedFileLeafNode,
) -> Result<HgAugmentedFileLeafNode> {
    // The augmented leaf carries content hashes but not a `ContentId`, so
    // recover it (and the canonical content size) from the source step-parent's
    // filenode envelope — the same envelope `resolve_conflict` loads to read
    // content identity.
    let envelope = HgFileNodeId::new(src.filenode)
        .load(ctx, blobstore)
        .await
        .with_context(|| format!("loading step-parent filenode envelope for {path}"))?;

    let contents = ContentBlobMeta {
        id: envelope.content_id(),
        size: envelope.content_size(),
        copy_from: None,
    };
    let (filenode_id, _) = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: UploadHgFileContents::ContentUploaded(contents),
        p1: None,
        p2: None,
    }
    .upload_with_path(ctx.clone(), blobstore.clone(), path.clone())
    .await?;

    Ok(HgAugmentedFileLeafNode {
        file_type: src.file_type,
        filenode: filenode_id.into_nodehash(),
        content_blake3: src.content_blake3,
        content_sha1: src.content_sha1,
        total_size: src.total_size,
        // Fresh parentless filenode with no copy metadata — no header bytes,
        // matching what `resolve_conflict` produces on the reference path.
        file_header_metadata: None,
    })
}

/// Compute the Mercurial node id for an augmented-manifest directory by
/// streaming the entries through `calculate_hg_node_id_stream`.
///
/// This intentionally avoids materialising the full directory in memory: each
/// entry is serialised to its hg-manifest line and fed straight into the sha1
/// state. Peak memory is O(1) per directory regardless of how many entries
/// the directory has, which matters a lot for huge directories like
/// `fbcode/third-party` where the previous `try_collect`-based path could OOM
/// or stampede the blobstore.
pub async fn compute_hg_node_id(
    node: ShardedMapV2Node<HgAugmentedManifestEntry>,
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    parents: &HgParents,
) -> Result<HgNodeHash> {
    let lines = node
        .into_entries(ctx, blobstore)
        .and_then(|(path, entry)| async move {
            let path = MPathElement::from_smallvec(path)?;
            serialize_manifest_entry(&path, &entry)
        });
    calculate_hg_node_id_stream(lines, parents).await
}

/// A reusable parent envelope for a merged directory.
#[derive(Debug, PartialEq, Eq)]
pub struct ReusableParentEnvelope {
    pub id: HgAugmentedManifestId,
    pub envelope: HgAugmentedManifestEnvelope,
}

/// Decision for a merged directory's augmented-manifest envelope.
#[derive(Debug, PartialEq, Eq)]
pub enum ParentEnvelopeReuse {
    Reuse(ReusableParentEnvelope),
    CreateFresh { computed_node_id: HgNodeHash },
}

/// Transient context propagated by `derive_manifest` from callbacks to their
/// parent tree callback.
#[derive(Debug)]
enum AugmentedDeriveContext {
    /// A leaf was freshly created or resolved. The leaf value itself carries all
    /// data the parent tree needs, so there is no extra directory metadata.
    Leaf,
    /// A directory was freshly created or re-emitted. Propagate the envelope
    /// metadata so the parent tree can embed it without reloading the child.
    Directory {
        augmented_manifest_id: Blake3,
        augmented_manifest_size: u64,
    },
}

/// Leaf callback for derive_manifest: creates an HgAugmentedFileLeafNode
/// for a new or changed file.
///
/// Parents are wrapped in `Traced<ParentIndex, _>` so we can recover the
/// original bonsai parent index after `derive_manifest`'s value-only dedup. We
/// only feed p1/p2 filenodes to `store_file_change` because Mercurial
/// filenodes only encode (p1, p2) parentage; p3+ are ignored.
async fn create_augmented_leaf(
    ctx: CoreContext,
    blobstore: Arc<dyn KeyedBlobstore>,
    content_metadata_cache: Arc<HashMap<ContentId, ContentMetadataV2>>,
    copy_from_filenodes: Arc<HashMap<(NonRootMPath, ChangesetId), HgFileNodeId>>,
    parent_bonsai_csids: (Option<ChangesetId>, Option<ChangesetId>),
    leaf_info: LeafInfo<Traced<ParentIndex, HgAugmentedFileLeafNode>, TrackedFileChange>,
) -> Result<(
    AugmentedDeriveContext,
    Traced<ParentIndex, HgAugmentedFileLeafNode>,
)> {
    let change = match leaf_info.change {
        Some(change) => change,
        None => {
            // Leaves-only conflict: parents have the same file with identical
            // content but different filenodes. The resolver itself reads no
            // blobstore; only the step-parent-only case needs a write, to mint
            // a fresh parentless filenode (mirroring `resolve_conflict`).
            let leaf =
                match check_content_identical_at_parents(&leaf_info.path, &leaf_info.parents)? {
                    LeafConflictResolution::Reuse(leaf) => leaf,
                    LeafConflictResolution::MintFresh(src) => {
                        mint_parentless_leaf(&ctx, &blobstore, &leaf_info.path, &src).await?
                    }
                };
            return Ok((AugmentedDeriveContext::Leaf, Traced::generate(leaf)));
        }
    };

    // Pick the (p1, p2) parents using the index-aware filter, not positional
    // indexing: after `derive_manifest`'s value-only dedup the surviving entry
    // for an octopus merge can be the wrong parent positionally (e.g. (X, X, Y)
    // collapses to [Traced(0,X), Traced(2,Y)], which is `(X, None)` for hg —
    // not `(X, Y)`).
    let (p1_leaf, p2_leaf) = hg_parents(&leaf_info.parents);
    let p1 = p1_leaf.map(|p| HgFileNodeId::new(p.filenode));
    let p2 = p2_leaf.map(|p| HgFileNodeId::new(p.filenode));

    let is_parent = |csid: &ChangesetId| {
        parent_bonsai_csids.0.as_ref() == Some(csid) || parent_bonsai_csids.1.as_ref() == Some(csid)
    };

    let copy_from = change
        .copy_from()
        .filter(|(_, copy_csid)| is_parent(copy_csid))
        .and_then(|(copy_path, copy_csid)| {
            copy_from_filenodes
                .get(&(copy_path.clone(), *copy_csid))
                .map(|filenode_id| (copy_path.clone(), *filenode_id))
        });

    // store_file_change is independent of get_metadata (the latter only
    // needs change.content_id()), so run them concurrently. store_file_change
    // also returns the file header metadata so we don't have to re-load the
    // freshly-stored filenode just to read it back.
    let store_fut = crate::derive_hg_changeset::store_file_change(
        ctx.clone(),
        blobstore.clone(),
        p1,
        p2,
        &leaf_info.path,
        &change,
        copy_from,
    );
    let metadata_fut = get_metadata(
        &ctx,
        &blobstore,
        &content_metadata_cache,
        change.content_id(),
    );
    let ((file_type, filenode_id, file_header_metadata), metadata) =
        future::try_join(store_fut, metadata_fut).await?;
    let is_metadata_present = !file_header_metadata.is_empty();

    let leaf = HgAugmentedFileLeafNode {
        file_type,
        filenode: filenode_id.into_nodehash(),
        content_blake3: metadata.seeded_blake3,
        content_sha1: metadata.sha1,
        total_size: metadata.total_size,
        file_header_metadata: is_metadata_present.then_some(file_header_metadata),
    };

    // Newly created leaf: no parent index. The context tells the parent tree
    // callback this entry came from the leaf callback.
    Ok((AugmentedDeriveContext::Leaf, Traced::generate(leaf)))
}

// ---------------------------------------------------------------------------
// Tree callback: types and helpers
// ---------------------------------------------------------------------------

/// Augmented subentries built up inside `create_augmented_tree`: either a
/// directly-placed entry (`Left`) or a reused parent subtree to be spliced in
/// by `ShardedMapV2Node::from_entries_and_partial_maps` (`Right`).
type AugmentedSubentriesMap =
    TrieMap<Either<HgAugmentedManifestEntry, LoadableShardedMapV2Node<HgAugmentedManifestEntry>>>;

type AugmentedTree = Traced<ParentIndex, HgAugmentedManifestId>;
type AugmentedLeaf = Traced<ParentIndex, HgAugmentedFileLeafNode>;
type AugmentedEntry = Entry<AugmentedTree, AugmentedLeaf>;

/// Raw shape emitted by `derive_manifest` for one tree subentry.
type RawAugmentedSubentry =
    Either<(Option<AugmentedDeriveContext>, AugmentedEntry), SortedVectorTrieMap<AugmentedEntry>>;

/// Shape of `tree_info.subentries` for the augmented-manifest tree callback.
type AugmentedTreeCallbackSubentries = TreeInfoSubentries<
    AugmentedTree,
    AugmentedLeaf,
    AugmentedDeriveContext,
    SortedVectorTrieMap<AugmentedEntry>,
>;

/// Semantic wrapper for the raw `derive_manifest` subentry shape.
enum AugmentedTreeSubentry {
    /// One child entry, with optional callback context if it was freshly
    /// produced by a child callback.
    Entry {
        ctx: Option<AugmentedDeriveContext>,
        entry: AugmentedEntry,
    },
    /// Whole parent subtree reused by `manifest::derive::merge_subentries`.
    ReusedSubtree(SortedVectorTrieMap<AugmentedEntry>),
}

impl From<RawAugmentedSubentry> for AugmentedTreeSubentry {
    fn from(value: RawAugmentedSubentry) -> Self {
        match value {
            Either::Left((ctx, entry)) => Self::Entry { ctx, entry },
            Either::Right(entries) => Self::ReusedSubtree(entries),
        }
    }
}

/// What `create_augmented_tree` needs to do with one entry handed to it by
/// `derive_manifest`.
#[derive(Debug)]
enum SubentryAction {
    /// Freshly built child directory: embed using metadata propagated via
    /// `Ctx` from the inner `create_augmented_tree` invocation.
    PlaceFreshDirectory {
        treenode: HgNodeHash,
        aug_id: Blake3,
        aug_size: u64,
    },
    /// Any leaf — fresh or reused. The framework's leaf type already carries
    /// the full `HgAugmentedFileLeafNode`, so we just embed it.
    PlaceLeaf(HgAugmentedFileLeafNode),
    /// Reused single child (`MergeResult::Reuse`): fetch the rich entry from
    /// the originating parent's sharded map.
    LookupSingle { parent: ParentIndex },
    /// Reused subtree: splice the parent's partial map straight into the new
    /// sharded map, with no per-child loads.
    LookupSubtree { parent: ParentIndex },
}

impl SubentryAction {
    /// Classify one `tree_info.subentries` value into the action it needs.
    fn classify(value: AugmentedTreeSubentry) -> Result<Self> {
        match value {
            AugmentedTreeSubentry::Entry {
                ctx,
                entry: Entry::Leaf(traced_leaf),
            } => match ctx {
                Some(AugmentedDeriveContext::Directory { .. }) => {
                    bail!("directory derive context attached to leaf entry")
                }
                Some(AugmentedDeriveContext::Leaf) | None => {
                    Ok(Self::PlaceLeaf(traced_leaf.into_untraced()))
                }
            },
            AugmentedTreeSubentry::Entry {
                ctx,
                entry: Entry::Tree(traced_id),
            } => match ctx {
                Some(AugmentedDeriveContext::Directory {
                    augmented_manifest_id,
                    augmented_manifest_size,
                }) => Ok(Self::PlaceFreshDirectory {
                    treenode: traced_id.into_untraced().into_nodehash(),
                    aug_id: augmented_manifest_id,
                    aug_size: augmented_manifest_size,
                }),
                Some(AugmentedDeriveContext::Leaf) => {
                    bail!("leaf derive context attached to tree entry")
                }
                None => {
                    let parent = traced_id
                        .id()
                        .copied()
                        .context("Reused single child should carry a ParentIndex")?;
                    Ok(Self::LookupSingle { parent })
                }
            },
            AugmentedTreeSubentry::ReusedSubtree(trie_map) => {
                // `manifest::derive::merge_subentries` only emits a reused subtree
                // when `parents.len() <= 1` at the prefix, so every `Traced`
                // entry inside agrees on the parent index. Sample the first.
                // The rest-agree check is `debug_assert!` only — walking the
                // whole subtree on every call costs O(children) on the hot
                // path, and the invariant is the framework's responsibility
                // to uphold.
                let mut entries = trie_map.into_iter();
                let Some((_, first)) = entries.next() else {
                    bail!("manifest::derive should not emit an empty reused subtree");
                };
                let parent = entry_parent_index(&first)
                    .context("Reused subtree entry should carry a ParentIndex")?;
                debug_assert!(
                    entries.all(|(_, e)| entry_parent_index(&e) == Some(parent)),
                    "single-parent invariant violated for reused subtree (see manifest::derive::merge_subentries)",
                );
                Ok(Self::LookupSubtree { parent })
            }
        }
    }
}

/// Extract the originating parent index from a `Traced`-wrapped entry.
fn entry_parent_index(entry: &AugmentedEntry) -> Option<ParentIndex> {
    match entry {
        Entry::Tree(t) => t.id().copied(),
        Entry::Leaf(l) => l.id().copied(),
    }
}

/// Result of partitioning `tree_info.subentries`:
/// * `direct` entries can be inserted into the new sharded map immediately.
/// * `lookups_by_parent` need one batched fetch per originating parent before
///   they can be merged into `direct`.
struct PartitionedSubentries {
    direct: AugmentedSubentriesMap,
    lookups_by_parent: HashMap<ParentIndex, TrieMap<LookupKind>>,
}

/// Walk `tree_info.subentries` and partition them into direct placements vs
/// per-parent lookups, dispatching on `SubentryAction`.
///
/// ACL handling: freshly built children look up `acl_map` for their per-path
/// ACL pointer; reused children keep the parent's stored ACL pointer. This is
/// safe because a directory's `AclManifestId` is a pure function of its own
/// subtree — see `acl_manifest::derive::create_acl_manifest`, where a node's
/// content is its own `.slacl` restriction plus its children, with no
/// inherited ancestor ACLs baked in. Therefore any path whose ACL pointer
/// changes must have had a `.slacl` change *within its own subtree*, which
/// surfaces as a `changes` entry on that path and forces `derive_manifest` to
/// rebuild it rather than reuse it (blocking both `Either::Right` emission and
/// `MergeResult::Reuse`; see `manifest::derive::merge_subentries`). Ancestor
/// or sibling `.slacl` changes thus can never leave a reused subtree carrying
/// a stale pointer.
fn partition_subentries(
    subentries: AugmentedTreeCallbackSubentries,
    tree_path: &MPath,
    acl_map: &HashMap<MPath, AclManifestId>,
) -> Result<PartitionedSubentries> {
    let mut direct = AugmentedSubentriesMap::default();
    let mut lookups_by_parent: HashMap<ParentIndex, TrieMap<LookupKind>> = HashMap::new();

    for (key, value) in subentries {
        let action = SubentryAction::classify(AugmentedTreeSubentry::from(value))?;
        // Mercurial's manifest format uses '\n' as line terminator and
        // '\x01' as the metadata delimiter, so neither byte may appear in
        // a path element. The legacy `create_hg_manifest` rejects these
        // explicitly; we mirror that here for any element name we'd
        // newly place. `LookupSubtree`'s key is a sharded-map prefix, not
        // an element name, and the names inside were already validated
        // when the parent was derived — skip the check there.
        if !matches!(action, SubentryAction::LookupSubtree { .. })
            && (key.contains(&b'\n') || key.contains(&b'\x01'))
        {
            bail!(
                "Cannot derive Hg augmented manifest for a path element containing newline ('\\n') or the '\\x01' control code"
            );
        }
        match action {
            SubentryAction::PlaceFreshDirectory {
                treenode,
                aug_id,
                aug_size,
            } => {
                // Stamp the per-path ACL pointer for the freshly built child.
                let elem = MPathElement::from_smallvec(key)?;
                let child_acl_id = acl_map.get(&tree_path.join_element(Some(&elem))).copied();
                direct.insert(
                    elem,
                    Either::Left(HgAugmentedManifestEntry::DirectoryNode(
                        HgAugmentedDirectoryNode {
                            treenode,
                            augmented_manifest_id: aug_id,
                            augmented_manifest_size: aug_size,
                            acl_manifest_directory_id: child_acl_id,
                        },
                    )),
                );
            }
            SubentryAction::PlaceLeaf(leaf) => {
                direct.insert(key, Either::Left(HgAugmentedManifestEntry::FileNode(leaf)));
            }
            SubentryAction::LookupSingle { parent } => {
                lookups_by_parent
                    .entry(parent)
                    .or_default()
                    .insert(key, LookupKind::Entry);
            }
            SubentryAction::LookupSubtree { parent } => {
                lookups_by_parent
                    .entry(parent)
                    .or_default()
                    .insert(key, LookupKind::PartialMap);
            }
        }
    }

    Ok(PartitionedSubentries {
        direct,
        lookups_by_parent,
    })
}

/// Resolve per-parent lookups in parallel. Loads each contributing parent's
/// envelope once and issues one batched `get_entries_and_partial_maps` against
/// its sharded map — the same primitive `derive_from_hg_manifest_and_parents`
/// uses. Returns the resolved entries ready to merge into `PartitionedSubentries::direct`.
async fn resolve_parent_lookups(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parents: &[Traced<ParentIndex, HgAugmentedManifestId>],
    lookups_by_parent: HashMap<ParentIndex, TrieMap<LookupKind>>,
) -> Result<AugmentedSubentriesMap> {
    if lookups_by_parent.is_empty() {
        return Ok(AugmentedSubentriesMap::default());
    }

    // `parents` is the deduped per-directory parent list;
    // `Traced::id()` retains each entry's original Bonsai parent index.
    let parents_by_idx: HashMap<ParentIndex, HgAugmentedManifestId> = parents
        .iter()
        .filter_map(|p| Some((p.id().copied()?, *p.untraced())))
        .collect();

    let resolved: Vec<TrieMap<_>> = stream::iter(lookups_by_parent)
        .map(|(parent_idx, lookup_keys)| {
            // Resolve the parent id synchronously (borrowing `parents_by_idx`)
            // so the async block only captures the `Copy` result, not the
            // map itself — required for the `FnMut` closure to be reentrant.
            let parent_id = parents_by_idx
                .get(&parent_idx)
                .copied()
                .with_context(|| format!("Parent {parent_idx:?} missing from tree_info.parents"));
            async move {
                let parent_id = parent_id?;
                // Note: `finalize_envelope`'s `try_reuse_parent_envelope` may
                // reload p1/p2 envelopes already loaded here. The cacheblob
                // layer typically serves the second read from memory, but
                // it's not free — ser/deser still happens. If profiling
                // flags this, pass pre-loaded envelopes down instead.
                let env = parent_id.load(ctx, blobstore).await?;
                env.augmented_manifest
                    .subentries
                    .get_entries_and_partial_maps(ctx, blobstore, lookup_keys, 100)
                    .await
            }
        })
        .buffer_unordered(4)
        .try_collect()
        .await?;

    Ok(resolved.into_iter().flatten().collect())
}

/// Given the fully-merged subentries for one tree node, attempt parent reuse
/// (mirroring `create_hg_manifest`) or otherwise compute a fresh `hg_node_id`
/// and store the envelope. Records the envelope's id in the restricted-paths
/// manifest-id store when the JK is on and the path is a restriction root.
/// Returns the metadata to propagate through `Ctx` plus the resulting
/// `HgAugmentedManifestId`.
async fn finalize_envelope(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    expected_root_hg_node_id: HgNodeHash,
    tree_path: &MPath,
    parents: &[Traced<ParentIndex, HgAugmentedManifestId>],
    subentries: ShardedMapV2Node<HgAugmentedManifestEntry>,
    dir_acl_id: Option<AclManifestId>,
    restricted_paths: &ArcRestrictedPathsConfigBased,
    restricted_paths_enabled: bool,
) -> Result<(
    AugmentedDeriveContext,
    Traced<ParentIndex, HgAugmentedManifestId>,
)> {
    let is_root = tree_path.is_root();
    if is_root {
        assert_root_acl_pointer_invariant(&dir_acl_id)?;
    }
    // Select p1/p2 by original Bonsai parent index, not by position after
    // `derive_manifest` deduplication. For octopus parent trees `(X, X, Y)`,
    // the deduped list can be `[p1:X, p3:Y]`; positional indexing would
    // incorrectly treat p3 as Mercurial p2.
    let (p1, p2) = hg_parents(parents);
    let p1_hash = p1.map(HgAugmentedManifestId::into_nodehash);
    let p2_hash = p2.map(HgAugmentedManifestId::into_nodehash);

    let own_parents = HgParents::new(p1_hash, p2_hash);

    // Mirror `create_hg_manifest` parent reuse: if merged contents match an
    // hg-relevant parent (and the parent's ACL pointer agrees), reuse that
    // parent's envelope instead of minting `hash(merged contents, current
    // parents)`. The guard uses the deduped parent count — a surviving p3
    // can make reuse necessary even when only p1 is hg-relevant. At root,
    // only reuse an envelope whose `hg_node_id` already matches the
    // caller's canonical/supplied root id.
    let reuse = if parents.len() > 1 {
        try_reuse_parent_envelope(ctx, blobstore, subentries.clone(), dir_acl_id, p1, p2).await?
    } else {
        ParentEnvelopeReuse::CreateFresh {
            computed_node_id: compute_hg_node_id(subentries.clone(), ctx, blobstore, &own_parents)
                .await?,
        }
    };
    let computed_node_id = match reuse {
        ParentEnvelopeReuse::Reuse(reused)
            if !is_root
                || reused.envelope.augmented_manifest.hg_node_id == expected_root_hg_node_id =>
        {
            return Ok((
                AugmentedDeriveContext::Directory {
                    augmented_manifest_id: reused.envelope.augmented_manifest_id,
                    augmented_manifest_size: reused.envelope.augmented_manifest_size,
                },
                Traced::generate(reused.id),
            ));
        }
        ParentEnvelopeReuse::Reuse(_) => {
            compute_hg_node_id(subentries.clone(), ctx, blobstore, &own_parents).await?
        }
        ParentEnvelopeReuse::CreateFresh { computed_node_id } => computed_node_id,
    };

    // Roots use the canonical id from `HgChangeset.manifestid()`; non-root
    // trees use the freshly computed manifest hash.
    let hg_node_id = if is_root {
        expected_root_hg_node_id
    } else {
        computed_node_id
    };

    let augmented_manifest = ShardedHgAugmentedManifest {
        hg_node_id,
        p1: p1_hash,
        p2: p2_hash,
        computed_node_id,
        subentries,
        acl_manifest_directory_id: dir_acl_id,
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
        && let Some(non_root_path) = tree_path.clone().into_optional_non_root_path()
        && restricted_paths.should_record_manifest_id_entry(&non_root_path)
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
            warn!("Failed to track restricted path at {tree_path}: {e}");
        }
    }

    // Return this envelope's metadata through `Ctx` so a parent directory can
    // embed this child without loading it again.
    Ok((
        AugmentedDeriveContext::Directory {
            augmented_manifest_id,
            augmented_manifest_size,
        },
        Traced::generate(hg_augmented_manifest_id),
    ))
}

/// Tree callback for `derive_manifest`: build and store one augmented-manifest
/// directory.
///
/// `derive_manifest` may deduplicate equal parent entries. Parents therefore
/// carry `Traced<ParentIndex, _>` so we can still identify the original Bonsai
/// p1/p2 when computing Mercurial manifest parents; p3+ do not participate
/// because Hg manifest nodes record at most two parents.
async fn create_augmented_tree(
    ctx: CoreContext,
    blobstore: Arc<dyn KeyedBlobstore>,
    expected_root_hg_node_id: HgNodeHash,
    restricted_paths: ArcRestrictedPathsConfigBased,
    restricted_paths_enabled: bool,
    acl_map: Arc<HashMap<MPath, AclManifestId>>,
    tree_info: TreeInfo<
        Traced<ParentIndex, HgAugmentedManifestId>,
        Traced<ParentIndex, HgAugmentedFileLeafNode>,
        AugmentedDeriveContext,
        SortedVectorTrieMap<
            Entry<
                Traced<ParentIndex, HgAugmentedManifestId>,
                Traced<ParentIndex, HgAugmentedFileLeafNode>,
            >,
        >,
    >,
) -> Result<(
    AugmentedDeriveContext,
    Traced<ParentIndex, HgAugmentedManifestId>,
)> {
    let dir_acl_id = acl_map.get(&tree_info.path).copied();

    let PartitionedSubentries {
        mut direct,
        lookups_by_parent,
    } = partition_subentries(tree_info.subentries, &tree_info.path, &acl_map)?;

    let resolved =
        resolve_parent_lookups(&ctx, &blobstore, &tree_info.parents, lookups_by_parent).await?;
    direct.extend(resolved);

    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, direct).await?;

    finalize_envelope(
        &ctx,
        &blobstore,
        expected_root_hg_node_id,
        &tree_info.path,
        &tree_info.parents,
        subentries,
        dir_acl_id,
        &restricted_paths,
        restricted_paths_enabled,
    )
    .await
}

/// Derive an augmented manifest directly from a bonsai changeset and parent
/// augmented manifests, bypassing HgManifest construction entirely.
pub async fn derive_augmented_manifest_from_bonsai<Store>(
    ctx: &CoreContext,
    blobstore: &Store,
    parents: Vec<HgAugmentedManifestId>,
    file_changes: Vec<(NonRootMPath, Option<TrackedFileChange>)>,
    parent_bonsai_csids: (Option<ChangesetId>, Option<ChangesetId>),
    content_metadata_cache: &HashMap<ContentId, ContentMetadataV2>,
    // Canonical root id from `HgChangeset.manifestid()`. Forced-hash roots
    // (`UploadHgNodeHash::Supplied`) differ from the computed content hash;
    // the envelope must land at this key for client lookups to succeed.
    expected_root_hg_node_id: HgNodeHash,
    restricted_paths: &ArcRestrictedPathsConfigBased,
    acl_root_overlay: Option<AclManifestId>,
) -> Result<HgAugmentedManifestId>
where
    Store: KeyedBlobstore + Clone + 'static,
{
    let restricted_paths_enabled = justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None,
        Some("hg_augmented_manifest_write"),
    );

    let acl_map = match acl_root_overlay {
        None => Arc::new(HashMap::new()),
        // A merge can rebuild a directory where its parents diverge even when no
        // file change touches that path: `derive_manifest_inner`'s bonsai
        // semantics recurse into and rebuild a tree when "all parents entries
        // are trees" with no change on the path. Such directories are not
        // reachable from `file_changes`, so the targeted descent below would
        // miss their ACL pointers. Fall back to the full ACL-tree walk for
        // merges; they are comparatively rare.
        Some(root_id) if parents.len() >= 2 => {
            Arc::new(pre_walk_acl_tree(ctx, blobstore, root_id).await?)
        }
        // Single-parent and root commits rebuild exactly the directories on the
        // ancestor paths of `file_changes`. Scope the overlay load to that
        // frontier so the cost is O(commit size), not O(total ACL tree size).
        Some(root_id) => {
            let target_dirs = changed_ancestor_dirs(&file_changes);
            Arc::new(targeted_acl_overlay_map(ctx, blobstore, root_id, &target_dirs).await?)
        }
    };

    let copy_from_parents: [Option<(ChangesetId, HgAugmentedManifestId)>; 2] = [
        parent_bonsai_csids.0.zip(parents.first().copied()),
        parent_bonsai_csids.1.zip(parents.get(1).copied()),
    ];
    let copy_from_filenodes =
        resolve_copy_from_filenodes(ctx, blobstore, &file_changes, &copy_from_parents).await?;

    let blobstore_arc: Arc<dyn KeyedBlobstore> = Arc::new(blobstore.clone());
    let content_metadata_arc = Arc::new(content_metadata_cache.clone());
    let copy_from_arc = Arc::new(copy_from_filenodes);
    let restricted_paths_arc = Arc::clone(restricted_paths);

    let parents_traced: Vec<Traced<ParentIndex, HgAugmentedManifestId>> = parents
        .into_iter()
        .enumerate()
        .map(|(i, m)| Traced::assign(ParentIndex(i), m))
        .collect();

    let root_parents_traced = parents_traced.clone();

    let root = derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents_traced,
        file_changes,
        std::iter::empty(),
        {
            cloned!(ctx, blobstore_arc, restricted_paths_arc, acl_map);
            move |tree_info| {
                cloned!(ctx, blobstore_arc, restricted_paths_arc, acl_map);
                create_augmented_tree(
                    ctx,
                    blobstore_arc,
                    expected_root_hg_node_id,
                    restricted_paths_arc,
                    restricted_paths_enabled,
                    acl_map,
                    tree_info,
                )
            }
        },
        {
            cloned!(ctx, blobstore_arc, content_metadata_arc, copy_from_arc);
            move |leaf_info| {
                cloned!(ctx, blobstore_arc, content_metadata_arc, copy_from_arc);
                create_augmented_leaf(
                    ctx,
                    blobstore_arc,
                    content_metadata_arc,
                    copy_from_arc,
                    parent_bonsai_csids,
                    leaf_info,
                )
            }
        },
    )
    .await?;

    match root {
        Some(traced_id) => Ok(traced_id.into_untraced()),
        None => {
            // Empty manifest — all files deleted. Create empty augmented manifest.
            let tree_info = TreeInfo {
                path: MPath::ROOT,
                parents: root_parents_traced,
                subentries: Default::default(),
            };
            let (_, traced_id) = create_augmented_tree(
                ctx.clone(),
                blobstore_arc,
                expected_root_hg_node_id,
                restricted_paths_arc,
                restricted_paths_enabled,
                acl_map,
                tree_info,
            )
            .await?;
            Ok(traced_id.into_untraced())
        }
    }
}

/// Try to reuse a parent's augmented-manifest envelope if its subentries
/// match `merged_subentries`. Checks `p1` first, then `p2`; the first parent
/// whose content matches wins (Mercurial's first-parent-wins convention).
///
/// Mirrors `derive_hg_manifest.rs::create_hg_manifest`'s reuse logic. The
/// content-equality test is done by streaming `merged_subentries` under the
/// candidate parent's own `(p1, p2)`: if that hash equals the parent's stored
/// `computed_node_id`, then the merged content serialises identically to the
/// parent's content and we can reuse the parent's envelope wholesale.
///
/// Computes the fresh-envelope node id in the same stream pass as the reuse
/// checks, so `CreateFresh` does not need a second sharded-map traversal.
pub async fn try_reuse_parent_envelope(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    merged_subentries: ShardedMapV2Node<HgAugmentedManifestEntry>,
    dir_acl_id: Option<AclManifestId>,
    p1: Option<HgAugmentedManifestId>,
    p2: Option<HgAugmentedManifestId>,
) -> Result<ParentEnvelopeReuse> {
    let fallback_node_parents = HgParents::new(
        p1.map(HgAugmentedManifestId::into_nodehash),
        p2.map(HgAugmentedManifestId::into_nodehash),
    );
    let mut reuse_candidates = Vec::new();
    let mut hash_parent_sets = Vec::new();

    for parent_id in [p1, p2].into_iter().flatten() {
        let parent_env = parent_id.load(ctx, blobstore).await?;
        // The ACL pointer rides on the envelope but is part of neither the
        // storage key (= `hg_node_id`) nor the content-addressed digest (see
        // `ShardedHgAugmentedManifest::write_content_addressed_prefix`, which
        // never serialises `acl_manifest_directory_id`). Content addressing
        // therefore can't distinguish two envelopes that differ only in their
        // ACL pointer, so we check it explicitly before reusing. In normal
        // derivation content-match already implies ACL-match (ACL is a pure
        // function of subtree content), so this is a safety net rather than a
        // case seen in practice
        if parent_env.augmented_manifest.acl_manifest_directory_id != dir_acl_id {
            continue;
        }
        hash_parent_sets.push(HgParents::new(
            parent_env.augmented_manifest.p1,
            parent_env.augmented_manifest.p2,
        ));
        reuse_candidates.push((parent_id, parent_env));
    }
    hash_parent_sets.push(fallback_node_parents);

    let lines =
        merged_subentries
            .into_entries(ctx, blobstore)
            .and_then(|(path, entry)| async move {
                let path = MPathElement::from_smallvec(path)?;
                serialize_manifest_entry(&path, &entry)
            });
    let mut node_ids_by_parent_set = calculate_hg_node_ids_multi(lines, &hash_parent_sets).await?;
    let fallback_computed_node_id = node_ids_by_parent_set
        .pop()
        .context("missing fallback hg node-id hash")?;

    let reusable_parent = reuse_candidates
        .into_iter()
        .zip(node_ids_by_parent_set)
        .find_map(|((parent_id, parent_env), candidate_node_id)| {
            if candidate_node_id == parent_env.augmented_manifest.computed_node_id {
                Some(ReusableParentEnvelope {
                    id: parent_id,
                    envelope: parent_env,
                })
            } else {
                None
            }
        });

    Ok(match reusable_parent {
        Some(reusable_parent) => ParentEnvelopeReuse::Reuse(reusable_parent),
        None => ParentEnvelopeReuse::CreateFresh {
            computed_node_id: fallback_computed_node_id,
        },
    })
}

pub async fn derive_from_full_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn KeyedBlobstore>,
    hg_manifest_id: HgManifestId,
    acl_root_overlay: Option<AclManifestId>,
) -> Result<HgAugmentedManifestId> {
    let predecessor = AclOverlayHgManifestId {
        hg_manifest_id,
        acl_manifest_id: acl_root_overlay,
    };
    let root_entry = derive_manifest_from_predecessor(
        ctx.clone(),
        blobstore.clone(),
        predecessor,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                cloned!(ctx, blobstore);
                async move {
                    let acl_manifest_directory_id = tree_info.predecessor.acl_manifest_id;
                    let hg_manifest = tree_info
                        .predecessor
                        .hg_manifest_id
                        .load(&ctx, &blobstore)
                        .await?;
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
                        hg_node_id: tree_info.predecessor.hg_manifest_id.into_nodehash(),
                        p1: hg_manifest.p1(),
                        p2: hg_manifest.p2(),
                        computed_node_id: hg_manifest.computed_node_id(),
                        subentries,
                        acl_manifest_directory_id,
                    };

                    assert_root_acl_pointer_invariant(
                        &augmented_manifest.acl_manifest_directory_id,
                    )?;

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
                        treenode: tree_info.predecessor.hg_manifest_id.into_nodehash(),
                        augmented_manifest_id,
                        augmented_manifest_size,
                        acl_manifest_directory_id,
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
                    .ok_or_else(|| anyhow!("Missing metadata for {filenode_id}"))?;
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

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Load an ACL manifest node by ID and build a map of direct child directory
/// name -> child AclManifestId. Only directory entries (waypoints and restriction
/// roots) are included; leaf entries (.slacl files) are skipped.
async fn load_acl_child_directory_map(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    acl_id: &AclManifestId,
) -> Result<HashMap<MPathElement, AclManifestId>> {
    let acl_manifest: AclManifest = blobstore::Loadable::load(acl_id, ctx, blobstore).await?;
    acl_manifest
        .into_subentries(ctx, blobstore)
        .try_filter_map(|(name, entry)| async move {
            match entry {
                AclManifestEntry::Directory(dir) => Ok(Some((name, dir.id))),
                AclManifestEntry::AclFile(_) => Ok(None),
            }
        })
        .try_collect()
        .await
}

/// The set of directories `derive_manifest` rebuilds for a single-parent (or
/// root) commit: every ancestor directory of every changed file path, plus the
/// root. `create_augmented_tree` only ever looks up the ACL overlay for a
/// rebuilt directory and that directory's immediate children, so this set bounds
/// the part of the ACL tree that `targeted_acl_overlay_map` must cover.
///
/// NOT valid for merges: those can additionally rebuild directories where the
/// parents diverge, which are not reachable from `file_changes` (see the merge
/// fallback in `derive_augmented_manifest_from_bonsai`).
fn changed_ancestor_dirs(
    file_changes: &[(NonRootMPath, Option<TrackedFileChange>)],
) -> HashSet<MPath> {
    let mut dirs = HashSet::new();
    dirs.insert(MPath::ROOT);
    for (path, _change) in file_changes {
        // `into_ancestors` yields the path itself then each ancestor directory
        // up to the root; `skip(1)` drops the file leaf, leaving only its
        // ancestor directories.
        for dir in MPath::from(path.clone()).into_ancestors().skip(1) {
            dirs.insert(dir);
        }
    }
    dirs
}

/// Build the `MPath -> AclManifestId` overlay map, scoped to the ACL directories
/// on the paths `derive_manifest` will rebuild (`target_dirs`).
///
/// Unlike [`pre_walk_acl_tree`], this descends only the branches that lead to a
/// rebuilt directory, so its cost is O(changed paths) rather than O(total ACL
/// tree size). For each visited (rebuilt) directory it records the directory's
/// own pointer and every immediate child's pointer — exactly the entries
/// `create_augmented_tree` queries (the directory itself at `:dir_acl_id`,
/// freshly-built children in `partition_subentries`, and exact subtree-copy
/// roots in `resolve_existing_directories`). Children outside `target_dirs` are
/// recorded but not descended into. Directories in `target_dirs` that have no
/// ACL node (no `.slacl` in their subtree) simply never surface as children and
/// are correctly absent from the map (`acl_map.get` -> `None`).
///
/// Only correct when the rebuilt set is fully determined by `target_dirs` — i.e.
/// single-parent or root commits, NOT merges (see the merge fallback in
/// `derive_augmented_manifest_from_bonsai`).
pub async fn targeted_acl_overlay_map(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    root_acl_id: AclManifestId,
    target_dirs: &HashSet<MPath>,
) -> Result<HashMap<MPath, AclManifestId>> {
    let nested: Vec<Vec<(MPath, AclManifestId)>> = bounded_traversal::bounded_traversal_stream(
        100,
        std::iter::once((MPath::ROOT, root_acl_id)),
        move |(path, acl_id): (MPath, AclManifestId)| {
            async move {
                let children = load_acl_child_directory_map(ctx, blobstore, &acl_id).await?;
                let mut entries = Vec::with_capacity(children.len() + 1);
                // The rebuilt directory's own ACL pointer.
                entries.push((path.clone(), acl_id));
                let mut to_descend = Vec::new();
                for (name, child_id) in children {
                    let child_path = path.join(std::iter::once(&name));
                    // Record every immediate child: a rebuilt parent stamps an
                    // `acl_manifest_directory_id` for each of its children.
                    entries.push((child_path.clone(), child_id));
                    // Descend only into children that are themselves rebuilt.
                    if target_dirs.contains(&child_path) {
                        to_descend.push((child_path, child_id));
                    }
                }
                Ok::<_, anyhow::Error>((entries, to_descend))
            }
            .boxed()
        },
    )
    .try_collect::<Vec<_>>()
    .await?;
    Ok(nested.into_iter().flatten().collect())
}

/// Walk the full ACL tree from the root and build a map of
/// `MPath -> AclManifestId` for every directory node. ACL nodes within the
/// walk are loaded concurrently via `bounded_traversal_stream`; sequential
/// load-per-node would otherwise make the walk's wall time scale with
/// `N * per-blob-latency`.
///
/// This loads the entire ACL tree regardless of commit size, so it is used only
/// where the rebuilt-directory set cannot be derived from the changed paths up
/// front — i.e. merge commits (see `derive_augmented_manifest_from_bonsai`).
/// Single-parent and root commits use [`targeted_acl_overlay_map`], which costs
/// O(commit size) instead.
pub(crate) async fn pre_walk_acl_tree(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    root_acl_id: AclManifestId,
) -> Result<HashMap<MPath, AclManifestId>> {
    bounded_traversal::bounded_traversal_stream(
        100,
        std::iter::once((MPath::ROOT, root_acl_id)),
        move |(path, acl_id): (MPath, AclManifestId)| {
            async move {
                let children = load_acl_child_directory_map(ctx, blobstore, &acl_id).await?;
                let child_nodes: Vec<(MPath, AclManifestId)> = children
                    .into_iter()
                    .map(|(name, child_id)| (path.join(std::iter::once(&name)), child_id))
                    .collect();
                Ok::<_, anyhow::Error>(((path, acl_id), child_nodes))
            }
            .boxed()
        },
    )
    .try_collect::<HashMap<_, _>>()
    .await
}

/// Assert that the root ACL pointer invariant holds: the root pointer must
/// never be `Some(canonical_empty_acl_manifest_id)`. Valid states are `None`
/// (no restrictions) or `Some(non_empty_id)` (root is a waypoint or restriction root).
fn assert_root_acl_pointer_invariant(pointer: &Option<AclManifestId>) -> Result<()> {
    if let Some(id) = pointer {
        anyhow::ensure!(
            *id != AclManifest::empty_id(),
            "Root acl_manifest_directory_id must never be Some(canonical_empty_acl_manifest_id). \
             This indicates broken sparse-tree normalization."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::hash::Sha1 as ContentSha1;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;

    use super::*;

    fn leaf(
        file_type: FileType,
        filenode_byte: u8,
        content_byte: u8,
        size: u64,
    ) -> HgAugmentedFileLeafNode {
        HgAugmentedFileLeafNode {
            file_type,
            filenode: HgNodeHash::new(NodeSha1::from_byte_array([filenode_byte; 20])),
            content_blake3: Blake3::from_byte_array([content_byte; 32]),
            content_sha1: ContentSha1::from_byte_array([content_byte; 20]),
            total_size: size,
            file_header_metadata: None,
        }
    }

    fn expect_reuse(resolution: LeafConflictResolution) -> Result<HgAugmentedFileLeafNode> {
        match resolution {
            LeafConflictResolution::Reuse(leaf) => Ok(leaf),
            other => Err(anyhow!("expected Reuse, got {other:?}")),
        }
    }

    fn expect_mint_fresh(resolution: LeafConflictResolution) -> Result<HgAugmentedFileLeafNode> {
        match resolution {
            LeafConflictResolution::MintFresh(leaf) => Ok(leaf),
            other => Err(anyhow!("expected MintFresh, got {other:?}")),
        }
    }

    /// `changed_ancestor_dirs` yields exactly the ancestor directories of every
    /// changed file (plus the root) and never the file leaf itself — this is the
    /// frontier the targeted ACL descent must cover for single-parent commits.
    #[mononoke::test]
    fn test_changed_ancestor_dirs() -> Result<()> {
        let dir = |s: &str| -> Result<MPath> { Ok(MPath::from(NonRootMPath::new(s)?)) };
        let change = |s: &str| -> Result<(NonRootMPath, Option<TrackedFileChange>)> {
            Ok((NonRootMPath::new(s)?, None))
        };

        // Nested file: every ancestor directory, root included, but not the leaf.
        let dirs = changed_ancestor_dirs(&[change("a/b/c.rs")?]);
        assert_eq!(
            dirs,
            [MPath::ROOT, dir("a")?, dir("a/b")?]
                .into_iter()
                .collect::<HashSet<_>>()
        );
        assert!(
            !dirs.contains(&dir("a/b/c.rs")?),
            "the changed file leaf must not be in the directory frontier"
        );

        // Root-level file: only the root is rebuilt.
        let dirs = changed_ancestor_dirs(&[change("README")?]);
        assert_eq!(dirs, [MPath::ROOT].into_iter().collect::<HashSet<_>>());

        // Multiple files (including a deletion): union of all ancestor dirs.
        let dirs =
            changed_ancestor_dirs(&[change("a/b/c.rs")?, change("a/x.rs")?, change("d/e.rs")?]);
        assert_eq!(
            dirs,
            [MPath::ROOT, dir("a")?, dir("a/b")?, dir("d")?]
                .into_iter()
                .collect::<HashSet<_>>()
        );

        Ok(())
    }

    #[mononoke::test]
    fn test_identical_content_returns_first_parent() -> Result<()> {
        // Given: p1 and p2 have the same content with different filenodes.
        let path = NonRootMPath::new("foo")?;
        let p1 = leaf(FileType::Regular, 0xAA, 0x42, 100);
        let p2 = leaf(FileType::Regular, 0xBB, 0x42, 100);
        let parents = vec![
            Traced::assign(ParentIndex(0), p1.clone()),
            Traced::assign(ParentIndex(1), p2),
        ];

        // When: resolving the leaves-only conflict.
        let resolved = expect_reuse(check_content_identical_at_parents(&path, &parents)?)?;

        // Then: the first hg-relevant parent leaf is reused.
        assert_eq!(resolved.filenode, p1.filenode, "should reuse p1's filenode");
        assert_eq!(resolved.content_blake3, p1.content_blake3);
        Ok(())
    }

    #[mononoke::test]
    fn test_different_content_bails() -> Result<()> {
        // Given: p1 and p2 disagree on content.
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(0), leaf(FileType::Regular, 0xAA, 0x42, 100)),
            Traced::assign(ParentIndex(1), leaf(FileType::Regular, 0xBB, 0x99, 100)),
        ];

        // When: resolving the leaves-only conflict.
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("different content must be unresolved");
        let msg = format!("{err:#}");

        // Then: the resolver rejects the content conflict.
        assert!(
            msg.contains("content conflict"),
            "expected content-conflict error, got: {msg}"
        );
        Ok(())
    }

    #[mononoke::test]
    fn test_different_file_type_bails() -> Result<()> {
        // Given: parents have the same content but different file types.
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(0), leaf(FileType::Regular, 0xAA, 0x42, 100)),
            Traced::assign(ParentIndex(1), leaf(FileType::Executable, 0xBB, 0x42, 100)),
        ];

        // When: resolving the leaves-only conflict.
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("different file_type must be unresolved");
        let msg = format!("{err:#}");

        // Then: the resolver rejects the observable manifest-entry conflict.
        assert!(
            msg.contains("file type conflict"),
            "expected file-type-conflict error, got: {msg}"
        );
        Ok(())
    }

    #[mononoke::test]
    fn test_only_p2_present_returns_p2_leaf() -> Result<()> {
        // Given: only p2 contains the file.
        let path = NonRootMPath::new("foo")?;
        let p2 = leaf(FileType::Regular, 0xBB, 0x42, 100);
        let parents = vec![Traced::assign(ParentIndex(1), p2.clone())];

        // When: resolving the leaves-only conflict.
        let resolved = expect_reuse(check_content_identical_at_parents(&path, &parents)?)?;

        // Then: p2 is still an hg-relevant parent and can be reused.
        assert_eq!(resolved.filenode, p2.filenode);
        Ok(())
    }

    #[mononoke::test]
    fn test_only_step_parents_mint_fresh() -> Result<()> {
        // Given: only step-parents carry the path, and their file content matches.
        let path = NonRootMPath::new("foo")?;
        let sp3 = leaf(FileType::Regular, 0xCC, 0x42, 100);
        let sp4 = leaf(FileType::Regular, 0xDD, 0x42, 100);
        let parents = vec![
            Traced::assign(ParentIndex(2), sp3.clone()),
            Traced::assign(ParentIndex(3), sp4),
        ];

        // When: resolving the leaves-only conflict.
        let src = expect_mint_fresh(check_content_identical_at_parents(&path, &parents)?)?;

        // Then: the caller is told to mint a parentless filenode using the
        // step-parent content as the source.
        assert_eq!(
            src.content_sha1, sp3.content_sha1,
            "minted leaf must carry the step-parent content"
        );
        assert_eq!(src.file_type, sp3.file_type);
        Ok(())
    }

    #[mononoke::test]
    fn test_step_parents_disagree_bails() -> Result<()> {
        // Given: only step-parents carry the path, but they disagree on content.
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(2), leaf(FileType::Regular, 0xCC, 0x42, 100)),
            Traced::assign(ParentIndex(3), leaf(FileType::Regular, 0xDD, 0x99, 100)),
        ];

        // When: resolving the leaves-only conflict.
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("step-parents with differing content must be unresolved");
        let msg = format!("{err:#}");

        // Then: the resolver rejects the content conflict instead of minting.
        assert!(
            msg.contains("content conflict"),
            "expected content-conflict error, got: {msg}"
        );
        Ok(())
    }

    #[mononoke::test]
    fn test_step_parent_disagreement_bails() -> Result<()> {
        // Given: p1 and p2 agree on content, but a step-parent disagrees.
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(0), leaf(FileType::Regular, 0xAA, 0x42, 100)),
            Traced::assign(ParentIndex(1), leaf(FileType::Regular, 0xBB, 0x42, 100)),
            Traced::assign(ParentIndex(2), leaf(FileType::Regular, 0xCC, 0x99, 200)),
        ];

        // When: resolving the leaves-only conflict.
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("step-parent disagreement must be unresolved");
        let msg = format!("{err:#}");

        // Then: the resolver checks all contributing parents before reusing p1.
        assert!(
            msg.contains("content conflict"),
            "expected content-conflict error, got: {msg}"
        );
        Ok(())
    }
}
