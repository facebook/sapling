/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;
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
use manifest::ManifestComparison;
use manifest::ManifestOps;
use manifest::Span;
use manifest::Traced;
use manifest::derive_manifest_from_predecessor;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::ShardedHgAugmentedManifest;
use mercurial_types::calculate_hg_node_id_stream;
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
use mononoke_types::TrackedFileChange;
use mononoke_types::TrieMap;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::hash::Blake3;
use mononoke_types::sharded_map_v2::LookupKind;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestId;
use restricted_paths_common::ManifestType;
use restricted_paths_common::RestrictedPathManifestIdEntry;
use restricted_paths_common::RestrictedPathsConfigBased;
use tracing::warn;

use crate::acl_overlay_manifest::AclOverlayHgManifestId;
use crate::derive_hg_manifest::ParentIndex;

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

    stream::iter(paths_by_parent.into_iter().zip(parents.iter()))
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
                            Ok(Some(((non_root, *cs_id), HgFileNodeId::new(leaf.filenode))))
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

/// Resolve a leaves-only conflict for augmented manifest derivation.
///
/// When two parents in a merge have the same file at the same path with different
/// filenodes but identical content, `derive_manifest` calls the leaf callback with
/// `change: None`. We verify content identity using fields already present on the
/// augmented leaf nodes (no blobstore reads), then reuse the first hg-relevant
/// parent's leaf — matching `resolve_conflict` in `derive_hg_manifest.rs`.
///
/// Octopus merges (>2 parents) are supported: parents are tagged with their bonsai
/// `ParentIndex`, and we only consult those tagged as p1/p2 here. This matches what
/// `derive_hg_manifest.rs::hg_parents` does, and is required because Mercurial
/// filenodes only encode (p1, p2) parentage. Step-parents (p3+) are intentionally
/// ignored.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "called from create_augmented_leaf in a subsequent commit in the stack"
    )
)]
pub(crate) fn check_content_identical_at_parents(
    path: &NonRootMPath,
    parents: &[Traced<ParentIndex, HgAugmentedFileLeafNode>],
) -> Result<HgAugmentedFileLeafNode> {
    use crate::derive_hg_manifest::unique_or_nothing;

    // Restrict to hg-relevant parents (indices 0 and 1 in the bonsai parent list).
    // p3+ parents are invisible to Mercurial filenode parentage, so they must not
    // affect the result.
    let hg_relevant: Vec<&HgAugmentedFileLeafNode> = parents
        .iter()
        .filter_map(|t| match t.id() {
            Some(ParentIndex(0)) | Some(ParentIndex(1)) => Some(t.untraced()),
            _ => None,
        })
        .collect();

    if hg_relevant.is_empty() {
        // No p1 or p2 contained this path. We cannot reuse a filenode whose
        // linknode is not a Mercurial ancestor of the changeset, so this is an
        // unresolved conflict.
        bail!(
            "Unresolved leaf conflict at {path}: file is present only in step-parents (p3+) of an octopus merge",
        );
    }

    unique_or_nothing(hg_relevant.iter().map(|p| p.file_type))
        .with_context(|| format!("Unresolved file type conflict at {path}"))?;

    // Compare on content_sha1 to match the legacy HgManifest derivation path,
    // which identifies content via the sha1 filenode-equivalent. total_size is
    // included as a cheap second guard.
    unique_or_nothing(hg_relevant.iter().map(|p| (p.content_sha1, p.total_size)))
        .with_context(|| format!("Unresolved content conflict at {path}"))?;

    // Content is identical across hg-relevant parents. Reuse the first
    // hg-relevant parent's complete leaf (filenode, content hashes, metadata) —
    // same choice as resolve_conflict's hg_parents() returning p1's filenode.
    Ok(hg_relevant[0].clone())
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

    /// p1 and p2 carry the same content with different filenodes (e.g. different
    /// linknodes from independent branches). The resolver should accept and
    /// return the first hg-relevant parent's leaf — matching `resolve_conflict`
    /// in `derive_hg_manifest.rs::hg_parents` returning p1's filenode.
    #[mononoke::test]
    fn test_identical_content_returns_first_parent() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let p1 = leaf(FileType::Regular, 0xAA, 0x42, 100);
        let p2 = leaf(FileType::Regular, 0xBB, 0x42, 100);
        let parents = vec![
            Traced::assign(ParentIndex(0), p1.clone()),
            Traced::assign(ParentIndex(1), p2),
        ];
        let resolved = check_content_identical_at_parents(&path, &parents)?;
        assert_eq!(resolved.filenode, p1.filenode, "should reuse p1's filenode");
        assert_eq!(resolved.content_blake3, p1.content_blake3);
        Ok(())
    }

    /// Different `content_sha1` between p1 and p2 is an unresolved conflict —
    /// we cannot pick a winner without a bonsai change to disambiguate.
    #[mononoke::test]
    fn test_different_content_bails() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(0), leaf(FileType::Regular, 0xAA, 0x42, 100)),
            Traced::assign(ParentIndex(1), leaf(FileType::Regular, 0xBB, 0x99, 100)),
        ];
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("different content must be unresolved");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("content conflict"),
            "expected content-conflict error, got: {msg}"
        );
        Ok(())
    }

    /// Same content but different `file_type` is also unresolved — Mercurial
    /// stores file type in the manifest entry, so the result is observable.
    #[mononoke::test]
    fn test_different_file_type_bails() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let parents = vec![
            Traced::assign(ParentIndex(0), leaf(FileType::Regular, 0xAA, 0x42, 100)),
            Traced::assign(ParentIndex(1), leaf(FileType::Executable, 0xBB, 0x42, 100)),
        ];
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("different file_type must be unresolved");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("file type conflict"),
            "expected file-type-conflict error, got: {msg}"
        );
        Ok(())
    }

    /// Only p2 contains the file. The resolver should return p2's leaf —
    /// `hg_parents`' filter accepts indices 0 and 1, so p2 alone is enough.
    #[mononoke::test]
    fn test_only_p2_present_returns_p2_leaf() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let p2 = leaf(FileType::Regular, 0xBB, 0x42, 100);
        let parents = vec![Traced::assign(ParentIndex(1), p2.clone())];
        let resolved = check_content_identical_at_parents(&path, &parents)?;
        assert_eq!(resolved.filenode, p2.filenode);
        Ok(())
    }

    /// Only a step-parent (p3+) carries the file: there is no Mercurial
    /// filenode whose linknode is an ancestor of the merge changeset, so we
    /// must bail rather than reuse the step-parent's filenode.
    #[mononoke::test]
    fn test_only_step_parent_bails() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let parents = vec![Traced::assign(
            ParentIndex(2),
            leaf(FileType::Regular, 0xCC, 0x42, 100),
        )];
        let err = check_content_identical_at_parents(&path, &parents)
            .expect_err("step-parent-only file must be unresolved");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("step-parents"),
            "expected step-parents error, got: {msg}"
        );
        Ok(())
    }

    /// p1 and p2 agree on content while a step-parent (p3) carries different
    /// content — the resolver must ignore p3 and accept p1's leaf, otherwise
    /// the (X, X, Y) octopus shape would falsely report a conflict.
    #[mononoke::test]
    fn test_step_parent_disagreement_ignored() -> Result<()> {
        let path = NonRootMPath::new("foo")?;
        let p1 = leaf(FileType::Regular, 0xAA, 0x42, 100);
        let parents = vec![
            Traced::assign(ParentIndex(0), p1.clone()),
            Traced::assign(ParentIndex(1), leaf(FileType::Regular, 0xBB, 0x42, 100)),
            // p3 carries different content; must be ignored.
            Traced::assign(ParentIndex(2), leaf(FileType::Regular, 0xCC, 0x99, 200)),
        ];
        let resolved = check_content_identical_at_parents(&path, &parents)?;
        assert_eq!(resolved.filenode, p1.filenode);
        Ok(())
    }
}
