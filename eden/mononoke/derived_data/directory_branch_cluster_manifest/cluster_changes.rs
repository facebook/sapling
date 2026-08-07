/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Cluster change computation for Directory Branch Cluster Manifest.
//!
//! This module computes the required directory branch cluster updates based
//! on subtree operations and directory deletions in a changeset.
//!
//! # Cluster Model
//!
//! A cluster is a flat star topology: one **primary** (the canonical source directory)
//! and one or more **secondaries** (directories that were copied from or merged into
//! the primary). We do not support nested clusters—if B copies A, and C copies B,
//! the result is a single cluster with A as primary and both B and C as direct
//! secondaries of A.
//!
//! # Invariants
//!
//! 1. **Flat topology**: All secondaries point directly to the primary (no chains).
//!    Formally: if `X.primary = P`, then `P.primary` must be `None`.
//!
//! 2. **Bidirectional consistency**: If `X.primary = P`, then `X ∈ P.secondaries`.
//!
//! # How Operations Preserve Invariants
//!
//! **Copy (source → dest)**: The dest becomes a secondary. To maintain flatness,
//! if source is already a secondary, we don't make dest a secondary of source—we
//! make dest a secondary of source's primary. This ensures no chains form even
//! when copying from a secondary.
//!
//! **Merge (source → dest)**: If source and dest are already in the same
//! cluster, the merge does not change the cluster relationships.  Otherwise,
//! the dest's cluster absorbs source's cluster. Source becomes a secondary
//! of dest's primary. If source was itself a primary with its own
//! secondaries, those secondaries are re-parented to become secondaries of
//! dest's primary. If source was a secondary, its old primary also becomes a
//! secondary of dest's primary. The entire cluster that source belonged to is
//! absorbed.
//!
//! **Deletion of a primary**: When a primary P is deleted, we promote one of its
//! secondaries to become the new primary (preferring a copy destination from this
//! commit, otherwise the first secondary). All other secondaries are re-parented
//! to the new primary, and the new primary has its primary field cleared.
//!
//! Every operation either:
//! - Attaches a new secondary directly to an existing primary
//! - Re-parents secondaries to a different primary (flattening or absorption)
//! - Promotes a secondary to primary when the old primary is deleted
//!
//! We never set `X.primary = Y` where Y has a primary - we always resolve
//! to the primary first. This guarantees chains cannot form.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::TryStreamExt;
use futures::future::try_join_all;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::get_implicit_deletes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::directory_branch_cluster_manifest::ClusterMember;
use mononoke_types::directory_branch_cluster_manifest::DirectoryBranchClusterManifest;
use mononoke_types::subtree_change::SubtreeChange;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;

/// Accumulates cluster updates for a single path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClusterUpdate {
    /// Set this directory's primary - the path it was copied FROM.
    pub set_primary: Option<MPath>,
    /// If true, clear this directory's primary field.
    pub clear_primary: bool,
    /// Add to this directory's secondaries - paths that were copied FROM this directory.
    pub add_secondaries: Vec<MPath>,
    /// Remove from this directory's secondaries - paths that were deleted or reassigned.
    pub delete_secondaries: Vec<MPath>,
    /// If true, this path was deleted and should be removed from the manifest entirely.
    pub is_deleted: bool,
}

impl ClusterUpdate {
    /// Ensure deterministic serialization by sorting and deduping
    pub fn normalize(&mut self) {
        self.add_secondaries.sort();
        self.add_secondaries.dedup();
        self.delete_secondaries.sort();
        self.delete_secondaries.dedup();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtreeOperationType {
    /// Copy or DeepCopy: source → dest
    /// - dest.set_primary = source (dest was copied FROM source)
    /// - source.add_secondaries.push(dest) (source was copied TO dest)
    Copy,
    /// Merge: source merges into dest
    /// - dest.add_secondaries.push(source) (dest receives content from source)
    /// - source.set_primary = dest (source was merged INTO dest)
    ///
    /// dest is the "primary" (surviving directory)
    Merge,
}

#[derive(Debug, Clone)]
pub struct SubtreeOperation {
    pub operation_type: SubtreeOperationType,
    pub dest_path: MPath,
    pub source_path: MPath,

    // If the source is part of a cluster, these are the existing cluster info.
    pub source_primary: Option<MPath>,
    pub source_secondaries: Vec<MPath>,

    // If source has a primary, these are the primary's secondaries.
    // This is needed to properly dissolve the old cluster when merging.
    pub source_primary_secondaries: Vec<MPath>,

    // If the dest is part of a cluster, this is the existing primary.
    pub dest_primary: Option<MPath>,
}

fn add_cluster_relationship(
    primary: MPath,
    secondary: MPath,
    cluster_changes: &mut HashMap<MPath, ClusterUpdate>,
) {
    cluster_changes
        .entry(secondary.clone())
        .or_default()
        .set_primary = Some(primary.clone());
    cluster_changes
        .entry(primary)
        .or_default()
        .add_secondaries
        .push(secondary.clone());
}

/// Build cluster changes from a list of subtree operations.
pub fn process_subtree_ops(operations: Vec<SubtreeOperation>) -> HashMap<MPath, ClusterUpdate> {
    let mut cluster_changes: HashMap<MPath, ClusterUpdate> = HashMap::new();

    for op in operations {
        // Skip same-path operations (e.g. `subtree merge --from-path X --to-path X`).
        // These are full-repo branch merges scoped to a directory and should not
        // create cluster relationships.
        if op.source_path == op.dest_path {
            continue;
        }

        match op.operation_type {
            SubtreeOperationType::Copy => {
                // COPY: dest was copied FROM source
                // If source already has a primary, flatten the chain:
                // dest.primary = source's primary (the root), not source itself
                // This ensures a single flat cluster with one root primary
                let root_primary = op.source_primary.clone().unwrap_or(op.source_path.clone());
                add_cluster_relationship(
                    root_primary.clone(),
                    op.dest_path.clone(),
                    &mut cluster_changes,
                );
            }
            SubtreeOperationType::Merge => {
                // MERGE: source was merged INTO dest
                // If source and dest are already in the same cluster, don't change
                // the existing relationship.  This prevents merging the primary into
                // a secondary from reversing the primary/secondary assignment.
                let already_clustered = {
                    // source is a secondary of dest
                    op.source_primary.as_ref() == Some(&op.dest_path)
                    // dest is a secondary of source
                    || op.dest_primary.as_ref() == Some(&op.source_path)
                    // both are secondaries of the same primary
                    || (op.source_primary.is_some()
                        && op.source_primary == op.dest_primary)
                };
                if already_clustered {
                    continue;
                }

                // If dest already has a primary (from a previous op in this commit),
                // flatten the cluster by using that root primary
                let root_primary = cluster_changes
                    .get(&op.dest_path)
                    .and_then(|u| u.set_primary.clone())
                    .unwrap_or(op.dest_path.clone());
                add_cluster_relationship(
                    root_primary.clone(),
                    op.source_path.clone(),
                    &mut cluster_changes,
                );

                // If source had a primary (was a secondary), that primary now becomes secondary of root
                if let Some(old_primary) = &op.source_primary {
                    // Don't add root_primary as a secondary of itself
                    if *old_primary != root_primary {
                        add_cluster_relationship(
                            root_primary.clone(),
                            old_primary.clone(),
                            &mut cluster_changes,
                        );
                    }

                    // The old primary's other secondaries also need to be transferred to the new root.
                    // This is necessary because the old primary is becoming a secondary itself,
                    // so it can no longer have secondaries (flat star topology invariant).
                    for secondary in &op.source_primary_secondaries {
                        // Don't add root_primary as a secondary of itself
                        // Also don't add source_path again (it's already added above)
                        if *secondary == root_primary || *secondary == op.source_path {
                            continue;
                        }
                        add_cluster_relationship(
                            root_primary.clone(),
                            secondary.clone(),
                            &mut cluster_changes,
                        );
                    }

                    // Clear the old primary's secondaries since it's now becoming a secondary
                    if !op.source_primary_secondaries.is_empty() {
                        cluster_changes
                            .entry(old_primary.clone())
                            .or_default()
                            .delete_secondaries
                            .extend(op.source_primary_secondaries.iter().cloned());
                    }
                }

                // Existing secondaries of source also become secondaries of root
                for secondary in &op.source_secondaries {
                    // Don't add root_primary as a secondary of itself
                    if *secondary == root_primary {
                        continue;
                    }
                    add_cluster_relationship(
                        root_primary.clone(),
                        secondary.clone(),
                        &mut cluster_changes,
                    );
                }

                // If source was a primary (had secondaries), clear those secondaries
                // since they've been transferred to the new root_primary
                if !op.source_secondaries.is_empty() {
                    cluster_changes
                        .entry(op.source_path.clone())
                        .or_default()
                        .delete_secondaries
                        .extend(op.source_secondaries.iter().cloned());
                }
            }
        }
    }

    cluster_changes
}

/// Information about a deleted directory that had cluster info.
#[derive(Debug, Clone)]
pub struct DeletedClusterPath {
    pub path: MPath,
    pub primary: Option<MPath>,
    pub secondaries: Option<Vec<MPath>>,
}

/// Find directories that were deleted and had cluster info.
///
/// This function handles two types of deletions:
/// 1. **Explicit deletes**: Files explicitly deleted in the changeset. Their ancestor
///    directories may become empty and need to be checked.
/// 2. **Implicit deletes**: When a file is added at a path that was previously a directory,
///    all contents of that directory (including nested directories) are implicitly deleted.
///    We use `get_implicit_deletes` to find all implicitly deleted file paths, then extract
///    their parent directories.
async fn find_deleted_cluster_paths(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    bonsai: &BonsaiChangeset,
    parent: &DirectoryBranchClusterManifest,
    subtree_destinations: &HashSet<MPath>,
) -> Result<Vec<DeletedClusterPath>> {
    use mononoke_types::FileChange;

    // Phase 1a: Handle implicit deletes using get_implicit_deletes
    // This gives us all FILE paths under directories that were replaced by files.
    let parent_cs_ids: Vec<_> = bonsai.parents().collect();

    // Collect paths added in this changeset
    let paths_added: Vec<NonRootMPath> = bonsai
        .file_changes()
        .filter_map(|(path, change)| match change {
            FileChange::Change(_) | FileChange::UntrackedChange(_) => Some(path.clone()),
            _ => None,
        })
        .collect();

    // Fetch parent skeleton manifests for implicit delete detection
    let parent_skeleton_manifests = try_join_all(parent_cs_ids.iter().map(|cs_id| async {
        let root = derivation_ctx
            .fetch_dependency::<RootSkeletonManifestV2Id>(ctx, *cs_id)
            .await?;
        root.into_inner_id()
            .load(ctx, derivation_ctx.blobstore())
            .await
    }))
    .await?;

    // Get all implicitly deleted file paths (files under replaced directories)
    let implicit_deleted_files: Vec<NonRootMPath> = get_implicit_deletes(
        ctx,
        derivation_ctx.blobstore().clone(),
        paths_added,
        parent_skeleton_manifests,
    )
    .try_collect()
    .await?;

    // Extract all ancestor directories from implicitly deleted files
    // These directories are definitely gone - no verification needed
    let implicit_deleted_dirs: HashSet<MPath> = implicit_deleted_files
        .iter()
        .flat_map(|file_path| {
            let mut dirs = Vec::new();
            let mut current = MPath::from(file_path.clone());
            while let Some((parent_path, _)) = current.split_dirname() {
                if !parent_path.is_root() {
                    dirs.push(parent_path.clone());
                }
                current = parent_path;
            }
            dirs
        })
        .collect();

    // Phase 1b: Collect candidate directories from explicit file deletions
    let mut deleted_files: HashSet<MPath> = HashSet::new();
    let mut explicit_delete_candidates: HashSet<MPath> = HashSet::new();
    for (path, change) in bonsai.file_changes() {
        match change {
            FileChange::Deletion | FileChange::UntrackedDeletion => {
                // Add the file itself to deleted paths
                deleted_files.insert(MPath::from(path.clone()));

                // Add all ancestor directories of deleted files as candidates
                let mut current = MPath::from(path.clone());
                while let Some((parent_path, _)) = current.split_dirname() {
                    if parent_path.is_root() {
                        break;
                    }
                    explicit_delete_candidates.insert(parent_path.clone());
                    current = parent_path;
                }
            }
            _ => {}
        }
    }

    // Phase 2: Verify explicit delete candidates against child's skeleton manifest
    // (Only for explicit deletes - implicit deletes are already confirmed)
    let verified_explicit_deletes: HashSet<MPath> = if explicit_delete_candidates.is_empty() {
        HashSet::new()
    } else {
        let skeleton_manifest = derivation_ctx
            .fetch_unknown_dependency::<RootSkeletonManifestV2Id>(
                ctx,
                None,
                bonsai.get_changeset_id(),
            )
            .await?
            .into_inner_id()
            .load(ctx, derivation_ctx.blobstore())
            .await?;

        // Filter out directories that are subtree destinations (being replaced, not deleted)
        let candidates_to_check: Vec<_> = explicit_delete_candidates
            .iter()
            .filter(|c| !subtree_destinations.contains(*c))
            .cloned()
            .collect();

        let deletion_checks: Vec<_> = candidates_to_check
            .into_iter()
            .map(|candidate| {
                let skeleton_manifest = &skeleton_manifest;
                async move {
                    // A directory is deleted if nothing exists at the path or a file exists
                    let dir_exists = matches!(
                        skeleton_manifest
                            .find_entry(
                                ctx.clone(),
                                derivation_ctx.blobstore().clone(),
                                candidate.clone(),
                            )
                            .await?,
                        Some(Entry::Tree(_))
                    );
                    Ok::<_, anyhow::Error>(if dir_exists { None } else { Some(candidate) })
                }
            })
            .collect();

        try_join_all(deletion_checks)
            .await?
            .into_iter()
            .flatten()
            .collect()
    };

    // Phase 3: Combine implicit and explicit deleted directories
    let all_deleted_paths: HashSet<MPath> = implicit_deleted_dirs
        .into_iter()
        .chain(verified_explicit_deletes)
        .chain(deleted_files)
        .collect();

    if all_deleted_paths.is_empty() {
        return Ok(Vec::new());
    }

    // Phase 4: Extract cluster info from parent DBCM for each deleted path (dirs and files).
    // No nested expansion needed - implicit deletes already gave us all nested paths.
    // A single find_entries shares one traversal across every path, so a prefix that is
    // absent from the manifest prunes its whole subtree instead of being rewalked per path.
    parent
        .find_entries(
            ctx.clone(),
            blobstore.clone(),
            all_deleted_paths.into_iter().map(PathOrPrefix::Path),
        )
        .try_filter_map(|(path, entry)| async move {
            anyhow::Ok(match entry {
                Entry::Tree(dir) if dir.is_clustered() => Some(DeletedClusterPath {
                    path,
                    primary: dir.primary,
                    secondaries: dir.secondaries,
                }),
                Entry::Leaf(file) if file.is_clustered() => Some(DeletedClusterPath {
                    path,
                    primary: file.primary,
                    secondaries: file.secondaries,
                }),
                _ => None,
            })
        })
        .try_collect()
        .await
}

/// Find deleted paths with cluster info and generate appropriate ClusterUpdate entries.
fn process_deletions(
    deleted_paths: &[DeletedClusterPath],
    subtree_destinations: &HashSet<MPath>,
    cluster_changes: &mut HashMap<MPath, ClusterUpdate>,
) {
    for deleted in deleted_paths {
        // Mark this path as deleted so it's removed from the manifest
        cluster_changes
            .entry(deleted.path.clone())
            .or_default()
            .is_deleted = true;

        // If deleted path was a secondary (had a primary), remove it from its primary's secondaries
        if let Some(ref primary) = deleted.primary {
            cluster_changes
                .entry(primary.clone())
                .or_default()
                .delete_secondaries
                .push(deleted.path.clone());
        }

        // If deleted path was a primary (had secondaries), we need to reassign
        if let Some(ref secondaries) = deleted.secondaries {
            if !secondaries.is_empty() {
                // Check if deleted path was copied to a new location in this commit
                // If so, the new location becomes the primary
                let new_primary = subtree_destinations
                    .iter()
                    .find(|dest| {
                        // Check if this destination was copied FROM the deleted path
                        cluster_changes
                            .get(*dest)
                            .and_then(|u| u.set_primary.as_ref())
                            .is_some_and(|p| *p == deleted.path)
                    })
                    .cloned();

                let new_primary = new_primary.unwrap_or_else(|| {
                    // No copy destination found, promote first secondary
                    secondaries[0].clone()
                });

                // Update all secondaries to point to new primary
                for secondary in secondaries {
                    if *secondary != new_primary {
                        cluster_changes
                            .entry(secondary.clone())
                            .or_default()
                            .set_primary = Some(new_primary.clone());

                        // New primary gets this secondary
                        cluster_changes
                            .entry(new_primary.clone())
                            .or_default()
                            .add_secondaries
                            .push(secondary.clone());
                    }
                }

                // Clear the new primary's own primary (it's now the root)
                cluster_changes
                    .entry(new_primary.clone())
                    .or_default()
                    .clear_primary = true;
            }
        }
    }
}

/// Compute cluster changes from a bonsai changeset's subtree operations,
/// returning a map of paths to the cluster changes for that path.
pub async fn compute_cluster_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    bonsai: &BonsaiChangeset,
    parent: &DirectoryBranchClusterManifest,
) -> Result<HashMap<MPath, ClusterUpdate>> {
    let mut operations: Vec<SubtreeOperation> = Vec::new();

    // Collect subtree operations with their existing cluster info from merged parent
    for (dest_path, change) in bonsai.subtree_changes() {
        let dest_mpath = dest_path.clone();

        let (operation_type, from_path) = match change {
            SubtreeChange::SubtreeCopy(copy) => (SubtreeOperationType::Copy, &copy.from_path),
            SubtreeChange::SubtreeDeepCopy(copy) => (SubtreeOperationType::Copy, &copy.from_path),
            SubtreeChange::SubtreeMerge(merge) => (SubtreeOperationType::Merge, &merge.from_path),
            SubtreeChange::SubtreeImport(_) => {
                // Import from external repo - no cluster relationship
                continue;
            }
            SubtreeChange::SubtreeCrossRepoMerge(_) => {
                // Merge into external repo - no cluster relationship
                continue;
            }
        };

        let from_mpath = from_path.clone();

        // Skip same-path operations (e.g. `subtree merge --from-path X --to-path X`).
        // These are full-repo branch merges scoped to a directory and should not
        // create cluster relationships since the source and destination are the
        // same directory.
        if from_mpath == dest_mpath {
            continue;
        }

        // Get existing cluster info from the merged parent manifest
        let mut source_primary: Option<MPath> = None;
        let mut source_secondaries: Vec<MPath> = Vec::new();

        if let Some(Entry::Tree(dir)) = parent
            .find_entry(ctx.clone(), blobstore.clone(), from_mpath.clone())
            .await?
        {
            source_primary = dir.primary.clone();
            if let Some(ref secondaries) = dir.secondaries {
                source_secondaries = secondaries.clone();
            }
        }

        // If source has a primary, also look up that primary's secondaries.
        // This is needed to properly dissolve the old cluster when merging.
        let mut source_primary_secondaries: Vec<MPath> = Vec::new();
        if let Some(ref primary_path) = source_primary {
            if let Some(Entry::Tree(dir)) = parent
                .find_entry(ctx.clone(), blobstore.clone(), primary_path.clone())
                .await?
            {
                if let Some(ref secondaries) = dir.secondaries {
                    source_primary_secondaries = secondaries.clone();
                }
            }
        }

        let mut dest_primary: Option<MPath> = None;

        if let Some(Entry::Tree(dir)) = parent
            .find_entry(ctx.clone(), blobstore.clone(), dest_mpath.clone())
            .await?
        {
            dest_primary = dir.primary.clone();
        }

        operations.push(SubtreeOperation {
            operation_type,
            dest_path: dest_mpath,
            source_path: from_mpath,
            source_primary,
            source_secondaries,
            source_primary_secondaries,
            dest_primary,
        });
    }

    // Build cluster changes from subtree operations
    let mut cluster_changes = process_subtree_ops(operations);

    // Handle deletions
    let subtree_destinations: HashSet<MPath> = bonsai.subtree_changes().keys().cloned().collect();
    let deleted_paths = find_deleted_cluster_paths(
        ctx,
        derivation_ctx,
        blobstore,
        bonsai,
        parent,
        &subtree_destinations,
    )
    .await?;
    process_deletions(&deleted_paths, &subtree_destinations, &mut cluster_changes);

    // Normalize all updates
    for update in cluster_changes.values_mut() {
        update.normalize();
    }

    Ok(cluster_changes)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use derivation_queue_thrift::DerivationPriority;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use mononoke_macros::mononoke;
    use mononoke_types::directory_branch_cluster_manifest::DirectoryBranchClusterManifestEntry;
    use mononoke_types::sharded_map_v2::ShardedMapV2Node;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;

    use super::*;

    #[facet::container]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        CommitGraph,
        dyn CommitGraphWriter,
        RepoDerivedData,
        RepoBlobstore,
        FilestoreConfig,
        RepoIdentity,
    );

    fn mpath(s: &str) -> MPath {
        MPath::new(s).unwrap()
    }

    // Helper to create a copy operation
    fn copy_op(
        source: &str,
        dest: &str,
        source_primary: Option<&str>,
        source_secondaries: Vec<&str>,
    ) -> SubtreeOperation {
        SubtreeOperation {
            operation_type: SubtreeOperationType::Copy,
            dest_path: mpath(dest),
            source_path: mpath(source),
            source_primary: source_primary.map(mpath),
            source_secondaries: source_secondaries.into_iter().map(mpath).collect(),
            source_primary_secondaries: Vec::new(),
            dest_primary: None,
        }
    }

    // Helper to create a merge operation
    fn merge_op(
        source: &str,
        dest: &str,
        source_primary: Option<&str>,
        source_secondaries: Vec<&str>,
    ) -> SubtreeOperation {
        SubtreeOperation {
            operation_type: SubtreeOperationType::Merge,
            dest_path: mpath(dest),
            source_path: mpath(source),
            source_primary: source_primary.map(mpath),
            source_secondaries: source_secondaries.into_iter().map(mpath).collect(),
            source_primary_secondaries: Vec::new(),
            dest_primary: None,
        }
    }

    // Helper to create a merge operation with dest_primary
    fn merge_op_with_dest_primary(
        source: &str,
        dest: &str,
        source_primary: Option<&str>,
        source_secondaries: Vec<&str>,
        dest_primary: Option<&str>,
    ) -> SubtreeOperation {
        SubtreeOperation {
            operation_type: SubtreeOperationType::Merge,
            dest_path: mpath(dest),
            source_path: mpath(source),
            source_primary: source_primary.map(mpath),
            source_secondaries: source_secondaries.into_iter().map(mpath).collect(),
            source_primary_secondaries: Vec::new(),
            dest_primary: dest_primary.map(mpath),
        }
    }

    // Helper to create a merge operation with source_primary_secondaries
    fn merge_op_with_source_primary_secondaries(
        source: &str,
        dest: &str,
        source_primary: Option<&str>,
        source_secondaries: Vec<&str>,
        source_primary_secondaries: Vec<&str>,
    ) -> SubtreeOperation {
        SubtreeOperation {
            operation_type: SubtreeOperationType::Merge,
            dest_path: mpath(dest),
            source_path: mpath(source),
            source_primary: source_primary.map(mpath),
            source_secondaries: source_secondaries.into_iter().map(mpath).collect(),
            source_primary_secondaries: source_primary_secondaries.into_iter().map(mpath).collect(),
            dest_primary: None,
        }
    }

    #[mononoke::test]
    fn test_simple_copy() {
        // Copy A → B (source has no existing cluster members)
        let ops = vec![copy_op("a", "b", None, vec![])];

        let changes = process_subtree_ops(ops);

        // A should have B in secondaries (A was copied TO B)
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("b")]);
        assert!(update_a.set_primary.is_none()); // A is the source, not a copy

        // B should have primary = A (B was copied FROM A)
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));
        assert!(update_b.add_secondaries.is_empty()); // B has no secondaries yet
    }

    #[mononoke::test]
    fn test_copy_with_existing_cluster() {
        // A already has cluster member X (X is a secondary of A)
        // Copy A → B: In hierarchical model, X is a sibling of B, not affected
        let ops = vec![copy_op("a", "b", None, vec!["x"])];

        let changes = process_subtree_ops(ops);

        // A should have B in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("b")]);

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));

        // X is not affected (it's a sibling, not in the copy path)
        assert!(!changes.contains_key(&mpath("x")));
    }

    #[mononoke::test]
    fn test_copy_multiple() {
        // Copy A → B and A → C in same commit
        let ops = vec![
            copy_op("a", "b", None, vec![]),
            copy_op("a", "c", None, vec![]),
        ];

        let changes = process_subtree_ops(ops);

        // A should have B and C in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries.len(), 2);
        assert!(update_a.add_secondaries.contains(&mpath("b")));
        assert!(update_a.add_secondaries.contains(&mpath("c")));

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));

        // C should have primary = A
        let update_c = changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_copy_duplicate_secondary() {
        // Copy B → A and C → A in same commit
        // Note: A receives two primaries - last one wins
        let ops = vec![
            copy_op("b", "a", None, vec![]),
            copy_op("c", "a", None, vec![]),
        ];

        let changes = process_subtree_ops(ops);

        // A should have primary = C (last one wins)
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.set_primary, Some(mpath("c")));

        // B should have A in secondaries
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.add_secondaries, vec![mpath("a")]);

        // C should have A in secondaries
        let update_c = changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.add_secondaries, vec![mpath("a")]);
    }

    #[mononoke::test]
    fn test_copy_chain() {
        // Copy A → B and B → C in same commit
        // Flat cluster model: A is the root primary, B and C are both secondaries of A
        let ops = vec![
            copy_op("a", "b", None, vec![]),
            copy_op("b", "c", Some("a"), vec![]),
        ];

        let changes = process_subtree_ops(ops);

        // A should have both B and C in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert!(update_a.add_secondaries.contains(&mpath("b")));
        assert!(update_a.add_secondaries.contains(&mpath("c")));
        assert_eq!(update_a.add_secondaries.len(), 2);

        // B should have primary = A, no secondaries
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));
        assert!(update_b.add_secondaries.is_empty());

        // C should have primary = A (flattened to root, not B)
        let update_c = changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_merge_simple() {
        // Merge B into A - A is the primary
        let ops = vec![merge_op("b", "a", None, vec![])];

        let changes = process_subtree_ops(ops);

        // A should have B in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("b")]);
        assert!(update_a.set_primary.is_none()); // A is the primary, not a secondary

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));
        assert!(update_b.add_secondaries.is_empty());
    }

    #[mononoke::test]
    fn test_merge_with_existing_cluster() {
        // B has existing secondaries X and Y (B is their primary)
        // Merge B into A should make A know about X and Y too
        let ops = vec![merge_op("b", "a", None, vec!["x", "y"])];

        let changes = process_subtree_ops(ops);

        // A should have B, X, Y in secondaries (A is the primary)
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries.len(), 3);
        assert!(update_a.add_secondaries.contains(&mpath("b")));
        assert!(update_a.add_secondaries.contains(&mpath("x")));
        assert!(update_a.add_secondaries.contains(&mpath("y")));

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));

        // X should have primary = A (transitive: X was in B's cluster, now A is primary)
        let update_x = changes.get(&mpath("x")).unwrap();
        assert_eq!(update_x.set_primary, Some(mpath("a")));

        // Y should have primary = A
        let update_y = changes.get(&mpath("y")).unwrap();
        assert_eq!(update_y.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_merge_multiple() {
        // Merge B into A and C into A
        let ops = vec![
            merge_op("b", "a", None, vec![]),
            merge_op("c", "a", None, vec![]),
        ];

        let changes = process_subtree_ops(ops);

        // A should have B and C in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries.len(), 2);
        assert!(update_a.add_secondaries.contains(&mpath("b")));
        assert!(update_a.add_secondaries.contains(&mpath("c")));

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));

        // C should have primary = A
        let update_c = changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_copy_then_merge() {
        // Copy A → B, then merge C into B
        // Flat cluster: A is root primary, B and C are both secondaries of A
        let ops = vec![
            copy_op("a", "b", None, vec![]),
            merge_op("c", "b", None, vec![]),
        ];

        let changes = process_subtree_ops(ops);

        // A should have both B and C in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert!(update_a.add_secondaries.contains(&mpath("b")));
        assert!(update_a.add_secondaries.contains(&mpath("c")));
        assert_eq!(update_a.add_secondaries.len(), 2);

        // B should have primary = A (from copy), no secondaries
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));
        assert!(update_b.add_secondaries.is_empty());

        // C should have primary = A (flattened to root, not B)
        let update_c = changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_merge_primary_into_secondary_noop() {
        // A is primary with secondary B (A.secondaries = [B], B.primary = A)
        // Merging A (source) into B (dest) should be a no-op since they're
        // already in the same cluster.
        let ops = vec![merge_op_with_dest_primary(
            "a",
            "b",
            None,      // source (A) has no primary - it IS the primary
            vec!["b"], // source (A) has B as a secondary
            Some("a"), // dest (B) has A as its primary
        )];

        let changes = process_subtree_ops(ops);

        // No cluster changes should be produced
        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_merge_secondary_into_primary_noop() {
        // A is primary with secondary B (A.secondaries = [B], B.primary = A)
        // Merging B (source) into A (dest) should be a no-op since they're
        // already in the same cluster.
        let ops = vec![merge_op_with_dest_primary(
            "b",
            "a",
            Some("a"), // source (B) has A as its primary
            vec![],    // source (B) has no secondaries
            None,      // dest (A) has no primary - it IS the primary
        )];

        let changes = process_subtree_ops(ops);

        // No cluster changes should be produced
        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_merge_siblings_in_same_cluster_noop() {
        // A is primary with secondaries B and C
        // Merging B (source) into C (dest) should be a no-op since they're
        // already in the same cluster (both secondaries of A).
        let ops = vec![merge_op_with_dest_primary(
            "b",
            "c",
            Some("a"), // source (B) has A as its primary
            vec![],    // source (B) has no secondaries
            Some("a"), // dest (C) also has A as its primary
        )];

        let changes = process_subtree_ops(ops);

        // No cluster changes should be produced
        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_copy_same_path_noop() {
        // Copy A → A (same path): should produce no cluster changes
        let ops = vec![copy_op("a", "a", None, vec![])];

        let changes = process_subtree_ops(ops);

        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_merge_same_path_noop() {
        // Merge A → A (same path): should produce no cluster changes
        let ops = vec![merge_op("a", "a", None, vec![])];

        let changes = process_subtree_ops(ops);

        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_merge_same_path_with_existing_cluster_noop() {
        // A is a primary with secondary B
        // Merge A → A (same path): should produce no cluster changes
        let ops = vec![merge_op("a", "a", None, vec!["b"])];

        let changes = process_subtree_ops(ops);

        assert!(changes.is_empty());
    }

    #[mononoke::test]
    fn test_merge_unrelated_still_creates_cluster() {
        // A and B are not in any cluster relationship
        // Merging B into A should still create a cluster with A as primary
        let ops = vec![merge_op_with_dest_primary(
            "b",
            "a",
            None,   // source (B) has no primary
            vec![], // source (B) has no secondaries
            None,   // dest (A) has no primary
        )];

        let changes = process_subtree_ops(ops);

        // A should have B in secondaries
        let update_a = changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("b")]);

        // B should have primary = A
        let update_b = changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a")));
    }

    #[mononoke::test]
    fn test_merge_cross_cluster() {
        // Two separate clusters: P1 primary with secondary S1,
        // P2 primary with secondary S2.
        // Merging P1 (source) into P2 (dest) should merge the clusters
        // with P2 remaining as the primary.
        let ops = vec![merge_op_with_dest_primary(
            "p1",
            "p2",
            None,       // source (P1) has no primary - it IS a primary
            vec!["s1"], // source (P1) has S1 as a secondary
            None,       // dest (P2) has no primary - it IS a primary
        )];

        let changes = process_subtree_ops(ops);

        // P2 should have P1 and S1 as new secondaries (S2 is already
        // in P2's cluster from the parent manifest, not part of changes)
        let update_p2 = changes.get(&mpath("p2")).unwrap();
        assert_eq!(update_p2.add_secondaries.len(), 2);
        assert!(update_p2.add_secondaries.contains(&mpath("p1")));
        assert!(update_p2.add_secondaries.contains(&mpath("s1")));
        assert!(update_p2.set_primary.is_none()); // P2 stays as the primary

        // P1 should become a secondary of P2 and lose its old secondaries
        let update_p1 = changes.get(&mpath("p1")).unwrap();
        assert_eq!(update_p1.set_primary, Some(mpath("p2")));
        assert_eq!(update_p1.delete_secondaries, vec![mpath("s1")]);

        // S1 should be re-parented to P2
        let update_s1 = changes.get(&mpath("s1")).unwrap();
        assert_eq!(update_s1.set_primary, Some(mpath("p2")));
    }

    #[mononoke::test]
    fn test_merge_secondary_cross_cluster() {
        // Two separate clusters:
        //   Cluster 1: P1 is primary with secondary S1
        //   Cluster 2: P2 is primary with secondary S2
        // Merging S1 (a secondary from cluster 1) into P2 (primary of cluster 2)
        // should absorb S1 and its old primary P1 into P2's cluster.
        let ops = vec![merge_op_with_dest_primary(
            "s1",       // source: S1 (secondary of P1)
            "p2",       // dest: P2 (primary of cluster 2)
            Some("p1"), // source's primary: P1 (S1 is a secondary)
            vec![],     // source's secondaries: none (S1 is a secondary, not a primary)
            None,       // dest's primary: none (P2 IS the primary)
        )];

        let changes = process_subtree_ops(ops);

        // P2 should have S1 and P1 as new secondaries
        // (S2 is already in P2's cluster from the parent manifest, not part of changes)
        let update_p2 = changes.get(&mpath("p2")).unwrap();
        assert_eq!(update_p2.add_secondaries.len(), 2);
        assert!(update_p2.add_secondaries.contains(&mpath("s1")));
        assert!(update_p2.add_secondaries.contains(&mpath("p1")));
        assert!(update_p2.set_primary.is_none()); // P2 stays as the primary

        // S1 should become a secondary of P2
        let update_s1 = changes.get(&mpath("s1")).unwrap();
        assert_eq!(update_s1.set_primary, Some(mpath("p2")));

        // P1 (the old primary of the source's cluster) should also become
        // a secondary of P2, since the entire source cluster is absorbed
        let update_p1 = changes.get(&mpath("p1")).unwrap();
        assert_eq!(update_p1.set_primary, Some(mpath("p2")));
    }

    #[mononoke::test]
    fn test_merge_secondary_cross_cluster_dissolves_old_cluster() {
        // Two separate clusters:
        //   Cluster 1: P1 is primary with secondaries S1 and S1b
        //   Cluster 2: P2 is primary with secondary S2
        // Merging S1 (a secondary from cluster 1) into P2 (primary of cluster 2)
        // should absorb the ENTIRE cluster 1 into P2's cluster:
        //   - S1 becomes a secondary of P2
        //   - P1 (old primary) becomes a secondary of P2
        //   - S1b (other secondary of P1) becomes a secondary of P2
        //   - P1's secondaries should be cleared
        let ops = vec![merge_op_with_source_primary_secondaries(
            "s1",              // source: S1 (secondary of P1)
            "p2",              // dest: P2 (primary of cluster 2)
            Some("p1"),        // source's primary: P1
            vec![],            // source's secondaries: none (S1 is not a primary)
            vec!["s1", "s1b"], // P1's secondaries: S1 and S1b
        )];

        let changes = process_subtree_ops(ops);

        // P2 should have S1, P1, and S1b as new secondaries
        let update_p2 = changes.get(&mpath("p2")).unwrap();
        assert_eq!(update_p2.add_secondaries.len(), 3);
        assert!(update_p2.add_secondaries.contains(&mpath("s1")));
        assert!(update_p2.add_secondaries.contains(&mpath("p1")));
        assert!(update_p2.add_secondaries.contains(&mpath("s1b")));
        assert!(update_p2.set_primary.is_none()); // P2 stays as the primary

        // S1 should become a secondary of P2
        let update_s1 = changes.get(&mpath("s1")).unwrap();
        assert_eq!(update_s1.set_primary, Some(mpath("p2")));

        // P1 should become a secondary of P2 AND have its secondaries deleted
        let update_p1 = changes.get(&mpath("p1")).unwrap();
        assert_eq!(update_p1.set_primary, Some(mpath("p2")));
        assert!(update_p1.delete_secondaries.contains(&mpath("s1")));
        assert!(update_p1.delete_secondaries.contains(&mpath("s1b")));

        // S1b should become a secondary of P2
        let update_s1b = changes.get(&mpath("s1b")).unwrap();
        assert_eq!(update_s1b.set_primary, Some(mpath("p2")));
    }

    // Helper to create a DeletedClusterPath
    fn deleted_path(
        path: &str,
        primary: Option<&str>,
        secondaries: Option<Vec<&str>>,
    ) -> DeletedClusterPath {
        DeletedClusterPath {
            path: mpath(path),
            primary: primary.map(mpath),
            secondaries: secondaries.map(|s| s.into_iter().map(mpath).collect()),
        }
    }

    #[mononoke::test]
    fn test_delete_secondary() {
        // B was a secondary of A (B.primary = A)
        // Deleting B should remove B from A's secondaries
        let deleted = vec![deleted_path("b", Some("a"), None)];
        let subtree_destinations = HashSet::new();
        let mut cluster_changes = HashMap::new();

        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // A should have B in delete_secondaries
        let update_a = cluster_changes.get(&mpath("a")).unwrap();
        assert_eq!(update_a.delete_secondaries, vec![mpath("b")]);
        assert!(update_a.set_primary.is_none());
        assert!(update_a.add_secondaries.is_empty());
    }

    #[mononoke::test]
    fn test_delete_primary_promotes_first_secondary() {
        // A was a primary with secondaries B and C
        // Deleting A should promote B to primary, C points to B
        let deleted = vec![deleted_path("a", None, Some(vec!["b", "c"]))];
        let subtree_destinations = HashSet::new();
        let mut cluster_changes = HashMap::new();

        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // B becomes the new primary (first secondary promoted)
        let update_b = cluster_changes.get(&mpath("b")).unwrap();
        assert!(update_b.clear_primary); // B is now the root
        assert_eq!(update_b.add_secondaries, vec![mpath("c")]);

        // C now points to B
        let update_c = cluster_changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("b")));
    }

    #[mononoke::test]
    fn test_delete_primary_with_copy_destination() {
        // A was a primary with secondaries B and C
        // A is being deleted but was copied to D in this commit
        // D should become the new primary
        let deleted = vec![deleted_path("a", None, Some(vec!["b", "c"]))];
        let subtree_destinations: HashSet<MPath> = vec![mpath("d")].into_iter().collect();

        // Simulate that D was copied from A (D.set_primary = A)
        let mut cluster_changes = HashMap::new();
        cluster_changes.insert(
            mpath("d"),
            ClusterUpdate {
                set_primary: Some(mpath("a")),
                clear_primary: false,
                add_secondaries: vec![],
                delete_secondaries: vec![],
                is_deleted: false,
            },
        );

        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // D becomes the new primary (copy destination)
        let update_d = cluster_changes.get(&mpath("d")).unwrap();
        assert!(update_d.clear_primary); // D's primary should be cleared since it's now the root
        assert!(update_d.add_secondaries.contains(&mpath("b")));
        assert!(update_d.add_secondaries.contains(&mpath("c")));

        // B and C now point to D
        let update_b = cluster_changes.get(&mpath("b")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("d")));

        let update_c = cluster_changes.get(&mpath("c")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("d")));
    }

    #[mononoke::test]
    fn test_delete_multiple_paths() {
        // Delete both B and C, where B was secondary of A
        // and C was secondary of A
        let deleted = vec![
            deleted_path("b", Some("a"), None),
            deleted_path("c", Some("a"), None),
        ];
        let subtree_destinations = HashSet::new();
        let mut cluster_changes = HashMap::new();

        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // A should have both B and C in delete_secondaries
        let update_a = cluster_changes.get(&mpath("a")).unwrap();
        assert!(update_a.delete_secondaries.contains(&mpath("b")));
        assert!(update_a.delete_secondaries.contains(&mpath("c")));
    }

    #[mononoke::test]
    fn test_delete_no_cluster_info() {
        // Path with no primary and no secondaries - still needs to be marked as deleted
        let deleted = vec![deleted_path("a", None, None)];
        let subtree_destinations = HashSet::new();
        let mut cluster_changes = HashMap::new();

        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // The path should be marked as deleted even if it has no cluster relationships
        let update_a = cluster_changes.get(&mpath("a")).unwrap();
        assert!(update_a.is_deleted);
        assert!(update_a.set_primary.is_none());
        assert!(update_a.add_secondaries.is_empty());
        assert!(update_a.delete_secondaries.is_empty());
    }

    /// Build a DirectoryBranchClusterManifest with specified cluster info for testing
    async fn build_test_manifest(
        ctx: &CoreContext,
        blobstore: &Arc<dyn KeyedBlobstore>,
        entries: Vec<(MPath, Option<MPath>, Option<Vec<MPath>>)>, // (path, primary, secondaries)
    ) -> Result<DirectoryBranchClusterManifest> {
        // For simplicity in tests, we'll build a flat manifest structure
        // with empty subdirectories that have the cluster info
        let mut root_entries = Vec::new();

        for (path, primary, secondaries) in entries {
            // For each path, we need to navigate the tree and create directories as needed
            let elements: Vec<_> = path.into_iter().collect();

            if elements.is_empty() {
                continue;
            }

            // Create a directory with the cluster info
            let dir = DirectoryBranchClusterManifest {
                subentries: ShardedMapV2Node::default(), // Empty directory
                primary,
                secondaries,
            };

            root_entries.push((
                elements[0].clone().to_smallvec(),
                DirectoryBranchClusterManifestEntry::Directory(dir),
            ));
        }

        // Build the root manifest from entries
        let subentries = if root_entries.is_empty() {
            ShardedMapV2Node::default()
        } else {
            ShardedMapV2Node::from_entries(ctx, blobstore, root_entries).await?
        };

        Ok(DirectoryBranchClusterManifest {
            subentries,
            secondaries: None,
            primary: None,
        })
    }

    #[mononoke::fbinit_test]
    async fn test_find_deleted_paths_explicit_deletion(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Create parent commit with directory structure, then explicitly delete all files
        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            "A-B",
            changes! {
                "A" => |c| c.add_file("a/b/file.txt", "content"),
                "B" => |c| c.delete_file("a/b/file.txt"),
            },
        )
        .await?;

        let _parent_cs_id = changesets["A"];
        let child_cs_id = changesets["B"];

        // Derive skeleton_manifest_v2 for child (needed by find_deleted_cluster_paths)
        repo.repo_derived_data()
            .derive::<RootSkeletonManifestV2Id>(&ctx, child_cs_id, DerivationPriority::LOW)
            .await?;

        // Build parent manifest with cluster info for a/b
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
        let parent_manifest = build_test_manifest(
            &ctx,
            &blobstore,
            vec![(MPath::new("a")?, Some(MPath::new("x/y")?), None)],
        )
        .await?;

        // Get derivation context and bonsai
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let bonsai = child_cs_id.load(&ctx, repo.repo_blobstore()).await?;

        // Call find_deleted_cluster_paths
        let subtree_destinations = HashSet::new();
        let deleted = find_deleted_cluster_paths(
            &ctx,
            &derivation_ctx,
            &blobstore,
            &bonsai,
            &parent_manifest,
            &subtree_destinations,
        )
        .await?;

        // Assert we found the deleted directory with correct cluster info
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, MPath::new("a")?);
        assert_eq!(deleted[0].primary, Some(MPath::new("x/y")?));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_deleted_paths_file_replaces_directory(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Create parent commit with directory structure
        // Use single-level directory so the test manifest can represent it
        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            "A-B",
            changes! {
                "A" => |c| c.add_file("a/file.txt", "content"),
                "B" => |c| c.add_file("a", "file replaces directory"),
            },
        )
        .await?;

        let _parent_cs_id = changesets["A"];
        let child_cs_id = changesets["B"];

        // Derive skeleton_manifest_v2 for child (needed by find_deleted_cluster_paths)
        repo.repo_derived_data()
            .derive::<RootSkeletonManifestV2Id>(&ctx, child_cs_id, DerivationPriority::LOW)
            .await?;

        // Build parent manifest with cluster info for directory "a"
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
        let parent_manifest = build_test_manifest(
            &ctx,
            &blobstore,
            vec![(MPath::new("a")?, Some(MPath::new("x/y")?), None)],
        )
        .await?;

        // Get derivation context and bonsai
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let bonsai = child_cs_id.load(&ctx, repo.repo_blobstore()).await?;

        // Call find_deleted_cluster_paths
        let subtree_destinations = HashSet::new();
        let deleted = find_deleted_cluster_paths(
            &ctx,
            &derivation_ctx,
            &blobstore,
            &bonsai,
            &parent_manifest,
            &subtree_destinations,
        )
        .await?;

        // Assert we found the deleted directory with correct cluster info
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, MPath::new("a")?);
        assert_eq!(deleted[0].primary, Some(MPath::new("x/y")?));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_deleted_paths_nested_directories(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Create parent commit with nested directory structure inside a top-level dir
        // Use single-level directory with cluster info, but nested contents
        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            "A-B",
            changes! {
                "A" => |c| c.add_file("a/b/file.txt", "content").add_file("a/c/file.txt", "more"),
                "B" => |c| c.add_file("a", "file replaces directory with nested dirs"),
            },
        )
        .await?;

        let _parent_cs_id = changesets["A"];
        let child_cs_id = changesets["B"];

        // Derive skeleton_manifest_v2 for child (needed by find_deleted_cluster_paths)
        repo.repo_derived_data()
            .derive::<RootSkeletonManifestV2Id>(&ctx, child_cs_id, DerivationPriority::LOW)
            .await?;

        // Build parent manifest with cluster info for directory "a"
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
        let parent_manifest = build_test_manifest(
            &ctx,
            &blobstore,
            vec![(MPath::new("a")?, Some(MPath::new("x/y")?), None)],
        )
        .await?;

        // Get derivation context and bonsai
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let bonsai = child_cs_id.load(&ctx, repo.repo_blobstore()).await?;

        // Call find_deleted_cluster_paths
        let subtree_destinations = HashSet::new();
        let deleted = find_deleted_cluster_paths(
            &ctx,
            &derivation_ctx,
            &blobstore,
            &bonsai,
            &parent_manifest,
            &subtree_destinations,
        )
        .await?;

        // Assert we found the deleted directory with correct cluster info
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, MPath::new("a")?);
        assert_eq!(deleted[0].primary, Some(MPath::new("x/y")?));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_and_process_deletions(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Create parent commit with directory, then delete it
        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            "A-B",
            changes! {
                "A" => |c| c.add_file("a/b/file.txt", "content"),
                "B" => |c| c.delete_file("a/b/file.txt"),
            },
        )
        .await?;

        let _parent_cs_id = changesets["A"];
        let child_cs_id = changesets["B"];

        // Derive skeleton_manifest_v2 for child
        repo.repo_derived_data()
            .derive::<RootSkeletonManifestV2Id>(&ctx, child_cs_id, DerivationPriority::LOW)
            .await?;

        // Build parent manifest: a/b is a primary with secondaries [c/d, e/f]
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
        let parent_manifest = build_test_manifest(
            &ctx,
            &blobstore,
            vec![(
                MPath::new("a")?,
                None,
                Some(vec![MPath::new("c/d")?, MPath::new("e/f")?]),
            )],
        )
        .await?;

        // Get derivation context and bonsai
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let bonsai = child_cs_id.load(&ctx, repo.repo_blobstore()).await?;

        // Find deleted paths
        let subtree_destinations = HashSet::new();
        let deleted = find_deleted_cluster_paths(
            &ctx,
            &derivation_ctx,
            &blobstore,
            &bonsai,
            &parent_manifest,
            &subtree_destinations,
        )
        .await?;

        // Process deletions
        let mut cluster_changes = HashMap::new();
        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // Verify:
        // - a is marked as deleted
        // - c/d becomes new primary (first secondary promoted)
        // - e/f is reassigned to c/d
        let update_a = cluster_changes.get(&MPath::new("a")?).unwrap();
        assert!(update_a.is_deleted);

        let update_cd = cluster_changes.get(&MPath::new("c/d")?).unwrap();
        assert!(update_cd.clear_primary); // c/d is now the root
        assert_eq!(update_cd.add_secondaries, vec![MPath::new("e/f")?]);

        let update_ef = cluster_changes.get(&MPath::new("e/f")?).unwrap();
        assert_eq!(update_ef.set_primary, Some(MPath::new("c/d")?));

        Ok(())
    }

    // =========================================================================
    // File cluster tests
    // =========================================================================

    /// Build a DirectoryBranchClusterManifest with both directory and file entries for testing.
    /// `dir_entries`: (path, primary, secondaries) for directories
    /// `file_entries`: (path, primary, secondaries) for files
    async fn build_test_manifest_with_files(
        ctx: &CoreContext,
        blobstore: &Arc<dyn KeyedBlobstore>,
        dir_entries: Vec<(MPath, Option<MPath>, Option<Vec<MPath>>)>,
        file_entries: Vec<(MPath, Option<MPath>, Option<Vec<MPath>>)>,
    ) -> Result<DirectoryBranchClusterManifest> {
        use mononoke_types::directory_branch_cluster_manifest::DirectoryBranchClusterManifestFile;

        let mut root_entries = Vec::new();

        // Add directory entries
        for (path, primary, secondaries) in dir_entries {
            let elements: Vec<_> = path.into_iter().collect();
            if elements.is_empty() {
                continue;
            }

            let dir = DirectoryBranchClusterManifest {
                subentries: ShardedMapV2Node::default(),
                primary,
                secondaries,
            };

            root_entries.push((
                elements[0].clone().to_smallvec(),
                DirectoryBranchClusterManifestEntry::Directory(dir),
            ));
        }

        // Add file entries
        for (path, primary, secondaries) in file_entries {
            let elements: Vec<_> = path.into_iter().collect();
            if elements.is_empty() {
                continue;
            }

            let file = DirectoryBranchClusterManifestFile {
                secondaries,
                primary,
            };

            root_entries.push((
                elements[0].clone().to_smallvec(),
                DirectoryBranchClusterManifestEntry::File(file),
            ));
        }

        let subentries = if root_entries.is_empty() {
            ShardedMapV2Node::default()
        } else {
            ShardedMapV2Node::from_entries(ctx, blobstore, root_entries).await?
        };

        Ok(DirectoryBranchClusterManifest {
            subentries,
            secondaries: None,
            primary: None,
        })
    }

    #[mononoke::test]
    fn test_file_cluster_simple_copy() {
        // Copy file A → B (source has no existing cluster members)
        // This tests that file clusters are created correctly
        let ops = vec![copy_op("a.txt", "b.txt", None, vec![])];

        let changes = process_subtree_ops(ops);

        // a.txt should have b.txt in secondaries (a.txt was copied TO b.txt)
        let update_a = changes.get(&mpath("a.txt")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("b.txt")]);
        assert!(update_a.set_primary.is_none()); // a.txt is the source, not a copy

        // b.txt should have primary = a.txt (b.txt was copied FROM a.txt)
        let update_b = changes.get(&mpath("b.txt")).unwrap();
        assert_eq!(update_b.set_primary, Some(mpath("a.txt")));
        assert!(update_b.add_secondaries.is_empty()); // b.txt has no secondaries yet
    }

    #[mononoke::test]
    fn test_file_cluster_extend() {
        // Extend an existing file cluster: a.txt already has b.txt as secondary,
        // now copy a.txt → c.txt
        let ops = vec![copy_op("a.txt", "c.txt", None, vec!["b.txt"])];

        let changes = process_subtree_ops(ops);

        // a.txt should have c.txt added to secondaries
        let update_a = changes.get(&mpath("a.txt")).unwrap();
        assert_eq!(update_a.add_secondaries, vec![mpath("c.txt")]);
        assert!(update_a.set_primary.is_none());

        // c.txt should have primary = a.txt
        let update_c = changes.get(&mpath("c.txt")).unwrap();
        assert_eq!(update_c.set_primary, Some(mpath("a.txt")));

        // b.txt is not affected (existing secondary, not part of this operation)
        assert!(!changes.contains_key(&mpath("b.txt")));
    }

    #[mononoke::fbinit_test]
    async fn test_find_and_process_file_deletions(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Create a commit with a file, then delete it
        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            "A-B",
            changes! {
                "A" => |c| c.add_file("x", "content"),
                "B" => |c| c.delete_file("x"),
            },
        )
        .await?;

        let _parent_cs_id = changesets["A"];
        let child_cs_id = changesets["B"];

        // Derive skeleton_manifest_v2 for child
        repo.repo_derived_data()
            .derive::<RootSkeletonManifestV2Id>(&ctx, child_cs_id, DerivationPriority::LOW)
            .await?;

        // Build parent manifest: file "x" is a primary with secondaries [y, z]
        let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
        let parent_manifest = build_test_manifest_with_files(
            &ctx,
            &blobstore,
            vec![], // no directories
            vec![(
                MPath::new("x")?,
                None,
                Some(vec![MPath::new("y")?, MPath::new("z")?]),
            )],
        )
        .await?;

        // Get derivation context and bonsai
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let bonsai = child_cs_id.load(&ctx, repo.repo_blobstore()).await?;

        // Find deleted paths
        let subtree_destinations = HashSet::new();
        let deleted = find_deleted_cluster_paths(
            &ctx,
            &derivation_ctx,
            &blobstore,
            &bonsai,
            &parent_manifest,
            &subtree_destinations,
        )
        .await?;

        // Process deletions
        let mut cluster_changes = HashMap::new();
        process_deletions(&deleted, &subtree_destinations, &mut cluster_changes);

        // Verify:
        // - x is marked as deleted
        // - y becomes new primary (first secondary promoted)
        // - z is reassigned to y
        let update_x = cluster_changes.get(&MPath::new("x")?).unwrap();
        assert!(update_x.is_deleted);

        let update_y = cluster_changes.get(&MPath::new("y")?).unwrap();
        assert!(update_y.clear_primary); // y is now the root
        assert_eq!(update_y.add_secondaries, vec![MPath::new("z")?]);

        let update_z = cluster_changes.get(&MPath::new("z")?).unwrap();
        assert_eq!(update_z.set_primary, Some(MPath::new("y")?));

        Ok(())
    }
}
