/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use filestore::fetch_concat;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use itertools::Either;
use itertools::EitherOrBoth;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use manifest::PathOrPrefix;
use manifest::TreeInfo;
use manifest::TreeInfoSubentries;
use manifest::derive_manifest;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestDirectoryEntry;
use mononoke_types::acl_manifest::AclManifestDirectoryRestriction;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::acl_manifest::AclManifestEntryBlob;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestEntryBlobId;
use mononoke_types::typed_hash::AclManifestId;
use restricted_paths_acl_file::parse_acl_file;
use vec1::vec1;

use crate::RootAclManifestId;

/// Explicit ACL file change type.
#[derive(Clone, Debug)]
enum AclFileChange {
    AddOrModify(ContentId),
    Delete,
}

/// Context carried by `derive_manifest` from child `create_tree`/`create_leaf`
/// to the parent's `create_tree`, providing pre-computed flags for
/// `AclManifestDirectoryEntry` without requiring a blobstore load.
#[derive(Clone, Copy, Debug, Default)]
struct AclManifestNodeInfo {
    is_restricted: bool,
    has_restricted_descendants: bool,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Derive an AclManifest using the `derive_manifest` primitive.
///
/// Handles all cases: root commits, single-parent, and merges.
/// Only `.slacl` file changes (explicit + implicit deletes) are fed to
/// `derive_manifest`; unchanged subtrees are reused automatically.
pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<RootAclManifestId>,
    known: Option<&HashMap<ChangesetId, RootAclManifestId>>,
) -> Result<RootAclManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let acl_file_name = derivation_ctx
        .restricted_paths()
        .config()
        .acl_file_name()
        .to_string();

    let parent_ids: Vec<AclManifestId> = parents.iter().map(|p| p.0).collect();

    // Collect .slacl file changes (explicit + implicit deletes).
    let changes =
        get_acl_file_changes(ctx, blobstore, &bonsai, &parent_ids, &acl_file_name).await?;

    // Get subtree changes (for subtree copies/merges).
    let subtree_changes =
        get_acl_manifest_subtree_changes(ctx, derivation_ctx, &bonsai, known).await?;

    // Convert AclFileChange -> derive_manifest's Option<ContentId> format.
    let derive_changes: Vec<(NonRootMPath, Option<ContentId>)> = changes
        .into_iter()
        .map(|(path, change)| match change {
            AclFileChange::AddOrModify(content_id) => (path, Some(content_id)),
            AclFileChange::Delete => (path, None),
        })
        .collect();

    let result = inner_derive(
        ctx,
        blobstore,
        parent_ids,
        derive_changes,
        subtree_changes,
        &acl_file_name,
    )
    .await?;

    match result {
        Some(id) => Ok(RootAclManifestId(id)),
        None => empty_root_acl_manifest_id(ctx, blobstore).await,
    }
}

/// Derive an AclManifest from scratch using BSSMV3 to find all `.slacl` files.
///
/// Used for backfilling via `DerivableUntopologically`. Finds all ACL files
/// in the repo and feeds them as synthetic additions to `derive_manifest`
/// with empty parents.
pub(crate) async fn derive_from_scratch(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
) -> Result<RootAclManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let cs_id = bonsai.get_changeset_id();

    let acl_file_name = derivation_ctx
        .restricted_paths()
        .config()
        .acl_file_name()
        .to_string();

    // Derive dependencies.
    let bssm_v3 = derivation_ctx
        .fetch_dependency::<RootBssmV3DirectoryId>(ctx, cs_id)
        .await?;
    let root_fsnode_id = derivation_ctx
        .fetch_dependency::<RootFsnodeId>(ctx, cs_id)
        .await?;

    // Find all ACL files via BSSMV3 and resolve their content IDs via Fsnodes.
    let acl_paths: Vec<NonRootMPath> = bssm_v3
        .find_files_filter_basenames(
            ctx,
            blobstore.clone(),
            vec![MPath::ROOT],
            EitherOrBoth::Left(vec1![acl_file_name.clone()]),
            None,
        )
        .await?
        .try_filter_map(|path| async move { Ok(NonRootMPath::try_from(path).ok()) })
        .try_collect()
        .await?;

    // Resolve content IDs for each .slacl file via Fsnodes (in parallel).
    let derive_changes: Vec<(NonRootMPath, Option<ContentId>)> = stream::iter(acl_paths)
        .map(|path| {
            cloned!(ctx, blobstore, root_fsnode_id);
            async move {
                let content_id = resolve_content_id_via_fsnode(
                    &ctx,
                    &blobstore,
                    &root_fsnode_id,
                    &MPath::from(path.clone()),
                )
                .await?;
                Ok::<_, anyhow::Error>((path, Some(content_id)))
            }
        })
        .buffer_unordered(100)
        .try_collect()
        .await?;

    // Call derive_manifest with empty parents.
    let result = inner_derive(
        ctx,
        blobstore,
        vec![],
        derive_changes,
        vec![],
        &acl_file_name,
    )
    .await?;

    match result {
        Some(id) => Ok(RootAclManifestId(id)),
        None => empty_root_acl_manifest_id(ctx, blobstore).await,
    }
}

// ---------------------------------------------------------------------------
// Core derivation via derive_manifest
// ---------------------------------------------------------------------------

async fn inner_derive(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parents: Vec<AclManifestId>,
    changes: Vec<(NonRootMPath, Option<ContentId>)>,
    subtree_changes: Vec<ManifestParentReplacement<AclManifestId, AclManifestRestriction>>,
    acl_file_name: &str,
) -> Result<Option<AclManifestId>> {
    type Leaf = AclManifestRestriction;
    type LeafChange = ContentId;
    type TreeId = AclManifestId;
    type Ctx = AclManifestNodeInfo;

    let acl_file_bytes: Vec<u8> = acl_file_name.as_bytes().to_vec();

    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        subtree_changes,
        {
            cloned!(ctx, blobstore, acl_file_bytes);
            move |info: TreeInfo<TreeId, Leaf, Ctx, LoadableShardedMapV2Node<AclManifestEntry>>| {
                cloned!(ctx, blobstore, acl_file_bytes);
                async move {
                    create_acl_manifest(ctx, blobstore, &acl_file_bytes, info.subentries).await
                }
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info: LeafInfo<Leaf, LeafChange>| {
                cloned!(ctx, blobstore);
                async move {
                    let content_id = match leaf_info.change {
                        Some(id) => id,
                        None => {
                            anyhow::bail!("ACL file leaf with no change and no parents")
                        }
                    };
                    let entry_blob_id =
                        store_acl_entry_from_content_id(&ctx, &blobstore, content_id).await?;
                    let restriction = AclManifestRestriction { entry_blob_id };
                    Ok((AclManifestNodeInfo::default(), restriction))
                }
            }
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// create_tree callback
// ---------------------------------------------------------------------------

/// Build an `AclManifest` node from `derive_manifest`'s subentries.
///
/// 1. Convert each subentry from `Entry<AclManifestId, AclManifestRestriction>`
///    to `AclManifestEntry`, using `Ctx` for directory flags when available
///    and loading the child manifest for boundary-reuse entries.
/// 2. Build the ShardedMapV2 via `from_entries_and_partial_maps`.
/// 3. Look up the ACL file in the built map to determine the restriction.
/// 4. Compute `has_restricted_descendants` by checking for Directory entries.
/// 5. Store the manifest blob and return the ID with node info.
async fn create_acl_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn KeyedBlobstore>,
    acl_file_name: &[u8],
    subentries: TreeInfoSubentries<
        AclManifestId,
        AclManifestRestriction,
        AclManifestNodeInfo,
        LoadableShardedMapV2Node<AclManifestEntry>,
    >,
) -> Result<(AclManifestNodeInfo, AclManifestId)> {
    // Compute has_restricted_descendants from the original subentries before
    // consuming them. This avoids loading the built ShardedMapV2 entries.
    // - Either::Left with Entry::Tree = a directory child → restricted descendants
    // - Either::Right = reused map chunk → check its rollup data
    let has_restricted_descendants = subentries.values().any(|entry_or_map| match entry_or_map {
        Either::Left((_, Entry::Tree(_))) => true,
        Either::Left((_, Entry::Leaf(_))) => false,
        Either::Right(reused_map) => reused_map.rollup_data().has_restricted,
    });

    // Convert subentries, loading child manifests for boundary-reuse entries.
    let acl_subentries: TrieMap<
        Either<AclManifestEntry, LoadableShardedMapV2Node<AclManifestEntry>>,
    > = stream::iter(subentries.into_iter())
        .then(|(prefix, entry_or_map)| {
            cloned!(ctx, blobstore);
            async move {
                let converted = match entry_or_map {
                    Either::Left((maybe_info, entry)) => {
                        let acl_entry =
                            convert_manifest_entry(&ctx, &blobstore, maybe_info, entry).await?;
                        Either::Left(acl_entry)
                    }
                    Either::Right(map) => Either::Right(map),
                };
                Ok::<_, anyhow::Error>((prefix, converted))
            }
        })
        .try_collect()
        .await?;

    // Build the ShardedMapV2.
    let subentries =
        ShardedMapV2Node::from_entries_and_partial_maps(&ctx, &blobstore, acl_subentries).await?;

    // Look up the ACL file to determine this directory's restriction.
    let restriction = match subentries.lookup(&ctx, &blobstore, acl_file_name).await? {
        Some(AclManifestEntry::AclFile(restriction)) => {
            AclManifestDirectoryRestriction::Restricted(restriction)
        }
        _ => AclManifestDirectoryRestriction::Unrestricted,
    };

    let is_restricted = matches!(restriction, AclManifestDirectoryRestriction::Restricted(_));

    // Store and return.
    let manifest = AclManifest {
        restriction,
        subentries,
    };
    let id = manifest.into_blob().store(&ctx, &blobstore).await?;

    Ok((
        AclManifestNodeInfo {
            is_restricted,
            has_restricted_descendants,
        },
        id,
    ))
}

/// Convert a `derive_manifest` `Entry` to an `AclManifestEntry`.
///
/// For `Entry::Leaf`: trivial conversion to `AclFile`.
/// For `Entry::Tree` with `Some(ctx)`: use pre-computed flags from child's `create_tree`.
/// For `Entry::Tree` with `None` (boundary reuse): load the child manifest.
async fn convert_manifest_entry(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    maybe_info: Option<AclManifestNodeInfo>,
    entry: Entry<AclManifestId, AclManifestRestriction>,
) -> Result<AclManifestEntry> {
    match entry {
        Entry::Leaf(restriction) => Ok(AclManifestEntry::AclFile(restriction)),
        Entry::Tree(id) => {
            let (is_restricted, has_restricted_descendants) = match maybe_info {
                Some(info) => (info.is_restricted, info.has_restricted_descendants),
                None => {
                    // Boundary reuse — load child manifest to get flags.
                    let manifest = id.load(ctx, blobstore).await?;
                    let is_restricted = matches!(
                        manifest.restriction,
                        AclManifestDirectoryRestriction::Restricted(_)
                    );
                    // Use rollup data to check for restricted descendants
                    // instead of loading all subentries.
                    let has_restricted_descendants =
                        manifest.subentries.rollup_data().has_restricted;
                    (is_restricted, has_restricted_descendants)
                }
            };
            Ok(AclManifestEntry::Directory(AclManifestDirectoryEntry {
                id,
                is_restricted,
                has_restricted_descendants,
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// File changes collection
// ---------------------------------------------------------------------------

/// Collect all `.slacl` file changes: explicit from bonsai + implicit deletes.
///
/// Implicit deletes occur when a non-ACL file addition replaces a directory
/// that contained `.slacl` files in a parent manifest.
async fn get_acl_file_changes(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    bonsai: &BonsaiChangeset,
    parent_ids: &[AclManifestId],
    acl_file_name: &str,
) -> Result<Vec<(NonRootMPath, AclFileChange)>> {
    let acl_file_bytes = acl_file_name.as_bytes();

    // Explicit .slacl changes from bonsai file_changes().
    let explicit_changes: Vec<(NonRootMPath, AclFileChange)> = bonsai
        .file_changes()
        .filter(|(path, _)| path.basename().as_ref() == acl_file_bytes)
        .map(|(path, change)| {
            let file_change = match change.simplify() {
                Some(simplified) => AclFileChange::AddOrModify(simplified.content_id()),
                None => AclFileChange::Delete,
            };
            (path.clone(), file_change)
        })
        .collect();

    // Implicit deletes: file additions (non-ACL) that replace directories
    // containing .slacl files in parent manifests.
    let addition_prefixes: Vec<PathOrPrefix> = bonsai
        .file_changes()
        .filter(|(path, change)| change.is_changed() && path.basename().as_ref() != acl_file_bytes)
        .map(|(path, _)| PathOrPrefix::Prefix(MPath::from(path.clone())))
        .collect();

    let implicit_deletes: Vec<(NonRootMPath, AclFileChange)> =
        if addition_prefixes.is_empty() || parent_ids.is_empty() {
            vec![]
        } else {
            // Check each parent manifest for .slacl leaves under replaced paths.
            let all_implicit: HashSet<NonRootMPath> = stream::iter(parent_ids.iter())
                .then(|parent_id| {
                    cloned!(ctx, blobstore);
                    let prefixes = addition_prefixes.clone();
                    async move {
                        parent_id
                            .find_entries(ctx, blobstore, prefixes)
                            .try_filter_map(|(path, entry)| async move {
                                match entry {
                                    Entry::Leaf(_) => Ok(NonRootMPath::try_from(path).ok()),
                                    Entry::Tree(_) => Ok(None),
                                }
                            })
                            .try_collect::<Vec<_>>()
                            .await
                    }
                })
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .flatten()
                .collect();

            all_implicit
                .into_iter()
                .map(|path| (path, AclFileChange::Delete))
                .collect()
        };

    Ok(explicit_changes
        .into_iter()
        .chain(implicit_deletes)
        .collect())
}

/// Collect subtree changes for subtree copies/merges.
async fn get_acl_manifest_subtree_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    known: Option<&HashMap<ChangesetId, RootAclManifestId>>,
) -> Result<Vec<ManifestParentReplacement<AclManifestId, AclManifestRestriction>>> {
    let copy_sources: Vec<_> = bonsai
        .subtree_changes()
        .iter()
        .filter_map(|(path, change)| {
            let (from_cs_id, from_path) = change.copy_source()?;
            Some((path.clone(), from_cs_id, from_path.clone()))
        })
        .collect();

    stream::iter(copy_sources)
        .then(|(path, from_cs_id, from_path)| {
            cloned!(ctx);
            let blobstore = derivation_ctx.blobstore().clone();
            async move {
                let root_id = derivation_ctx
                    .fetch_unknown_dependency::<RootAclManifestId>(&ctx, known, from_cs_id)
                    .await?
                    .into_inner_id();
                // The subtree copy source may not exist in the AclManifest if
                // the source path has no .slacl files. In that case, use an
                // empty replacement to clear any existing entries at the
                // destination path.
                let replacements = root_id
                    .find_entry(ctx, blobstore, from_path.clone())
                    .await?
                    .into_iter()
                    .collect();
                Ok(ManifestParentReplacement { path, replacements })
            }
        })
        .try_collect()
        .await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fetch file content by ContentId, parse as ACL file, store as
/// AclManifestEntryBlob, and return the blob ID.
async fn store_acl_entry_from_content_id(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    content_id: ContentId,
) -> Result<AclManifestEntryBlobId> {
    let bytes = fetch_concat(blobstore, ctx, content_id).await?;
    let file_contents = FileContents::new_bytes(bytes);
    let acl_file = parse_acl_file(&file_contents)?;

    let entry_blob = AclManifestEntryBlob {
        repo_region_acl: acl_file.repo_region_acl().to_string(),
        permission_request_group: acl_file.permission_request_group().map(|g| g.to_string()),
    };
    entry_blob.into_blob().store(ctx, blobstore).await
}

/// Resolve a file's ContentId via Fsnodes.
/// Only used by derive_from_scratch for backfilling.
async fn resolve_content_id_via_fsnode(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    root_fsnode_id: &RootFsnodeId,
    acl_path: &MPath,
) -> Result<ContentId> {
    let entry = root_fsnode_id
        .fsnode_id()
        .find_entry(ctx.clone(), blobstore.clone(), acl_path.clone())
        .await?
        .ok_or_else(|| anyhow::anyhow!("ACL file not found in fsnodes: {:?}", acl_path))?;

    match entry {
        Entry::Leaf(f) => Ok(*f.content_id()),
        Entry::Tree(_) => anyhow::bail!("expected file but found directory: {:?}", acl_path),
    }
}

async fn empty_root_acl_manifest_id(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
) -> Result<RootAclManifestId> {
    let empty = AclManifest::empty();
    let id = empty.into_blob().store(ctx, blobstore).await?;
    Ok(RootAclManifestId(id))
}
