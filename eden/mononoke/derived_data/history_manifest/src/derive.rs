/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::format_err;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::Storable;
use borrowed::borrowed;
use bounded_traversal::bounded_traversal;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use either::Either;
use futures::future;
use futures::future::FutureExt;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::PathTree;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;
use mononoke_types::history_manifest::HistoryManifestDeletedNode;
use mononoke_types::history_manifest::HistoryManifestDirectory;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::history_manifest::HistoryManifestFile;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::typed_hash::HistoryManifestDirectoryId;
use smallvec::SmallVec;

use crate::HistoryManifestDerivationError;
use crate::mapping::RootHistoryManifestDirectoryId;
use crate::merge_subtrees::merge_subtrees;

/// What the unfold decided for this path.
enum UnfoldAction {
    /// Reuse a parent entry unchanged.
    Reuse(HistoryManifestEntry),
    /// Create a live file node.
    CreateFile {
        content_id: ContentId,
        file_type: FileType,
        parents: Vec<HistoryManifestEntry>,
    },
    /// Create a deleted file node.
    CreateDeletedFile { parents: Vec<HistoryManifestEntry> },
    /// Merge file from multiple parents (live).
    MergeFile { parents: Vec<HistoryManifestEntry> },
    /// Merge file from multiple parents (deleted).
    MergeDeletedFile { parents: Vec<HistoryManifestEntry> },
    /// Recurse into directory. Fold will collect child results.
    RecurseDirectory {
        parents: Vec<HistoryManifestEntry>,

        /// Unchanged entries and subtrees reused from parents, keyed by
        /// byte prefix. Feeds directly into
        /// `ShardedMapV2Node::from_entries_and_partial_maps`.
        reused: Vec<(
            SmallVec<[u8; 24]>,
            Either<HistoryManifestEntry, LoadableShardedMapV2Node<HistoryManifestEntry>>,
        )>,
    },
}

/// Node for bounded_traversal.
struct UnfoldNode {
    // The individual element of the path for the current level of recursion.
    // For example if we're traversing a/b/c.txt and are at b, this will be
    // Some("b").
    path_element: Option<MPathElement>,
    // The full path at the current level of recursion.
    // For example if we're traversing a/b/c.txt and are at b, this will be
    // "a/b".
    path: MPath,
    // The changes for this path in the current changeset - there are three possibilities:
    // 1. None - the path was not changed in the current changeset. This will
    //    be the case for intermediate directories (i.e. if the changeset
    //    modifies a/b/c.txt, then when we visit nodes a and b, this field will
    //    be None.).
    // 2. Some(None) - the path was deleted in the current changeset.
    // 3. Some(Some(...) - the path was modified in the current changeset.
    change: Option<Option<(ContentId, FileType)>>,
    children: Vec<(
        MPathElement,
        PathTree<Option<Option<(ContentId, FileType)>>>,
    )>,
    // If we're at a directory, this will contain the manifest node for that
    // directory for each parent. Empty if we're at a file, even if a parent
    // had a directory at this path.
    parent_dirs: Vec<(ChangesetId, HistoryManifestDirectory)>,
    // The manifest entry for the current node for each parent.
    parent_entries: Vec<(ChangesetId, HistoryManifestEntry)>,
    /// When true, this node and all descendants are being implicitly deleted
    /// (i.e. a file replaced a parent directory). All entries should
    /// produce deletion nodes regardless of their live/deleted status in the
    /// parent manifest.
    implicit_deletion: bool,
}

fn is_not_directory(entry: &HistoryManifestEntry) -> bool {
    !matches!(entry, HistoryManifestEntry::Directory(_))
}

fn is_deleted_entry(entry: &HistoryManifestEntry) -> bool {
    matches!(entry, HistoryManifestEntry::DeletedNode(_))
}

/// Get the directory ID from an entry, if it is a directory variant.
fn get_directory_id(entry: &HistoryManifestEntry) -> Option<HistoryManifestDirectoryId> {
    match entry {
        HistoryManifestEntry::Directory(id) => Some(*id),
        _ => None,
    }
}

async fn store_blob<V: BlobstoreValue<Key: Copy + Send + Sync + 'static>>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    value: V,
) -> Result<V::Key> {
    let blob = value.into_blob();
    let id = *blob.id();
    blob.store(ctx, blobstore).await?;
    Ok(id)
}

async fn load_blob<Id: Loadable + Copy>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    id: Id,
) -> Result<Id::Value> {
    id.load(ctx, blobstore).await.map_err(Into::into)
}

/// Build a merged file node from multiple parents.
///
/// When parents disagree on a file entry (different content or
/// deleted-vs-live), we create a new file node that records all parents.
///
/// If the new node would be identical to an existing parent node
/// (same content, same parents list), it is reused to avoid creating a
/// duplicate blob.
async fn merge_live_file_node(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    cs_id: ChangesetId,
    path: &MPath,
    parents: Vec<HistoryManifestEntry>,
) -> Result<HistoryManifestEntry> {
    // Find the first File parent to get content_id/file_type.
    let first_file_id = parents
        .iter()
        .find_map(|p| match p {
            HistoryManifestEntry::File(id) => Some(*id),
            _ => None,
        })
        .ok_or_else(|| anyhow!("MergeFile requires at least one File parent"))?;
    let parent_file = load_blob(ctx, blobstore, first_file_id).await?;

    // Reuse heuristic: if the first parent already has matching fields.
    if parent_file.linknode != cs_id && parent_file.parents == parents {
        return Ok(HistoryManifestEntry::File(first_file_id));
    }

    let subentries = collect_parent_subentries(ctx, blobstore, &parents).await?;

    let file = HistoryManifestFile {
        parents,
        content_id: parent_file.content_id,
        file_type: parent_file.file_type,
        path_hash: path.get_path_hash(),
        linknode: cs_id,
        subentries,
    };
    let id = store_blob(ctx, blobstore, file).await?;
    Ok(HistoryManifestEntry::File(id))
}

/// Build a merged deleted node from multiple parents.
async fn merge_deleted_node(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    cs_id: ChangesetId,
    _path: &MPath,
    parents: Vec<HistoryManifestEntry>,
) -> Result<HistoryManifestEntry> {
    let subentries = collect_parent_subentries(ctx, blobstore, &parents).await?;
    let node = HistoryManifestDeletedNode {
        parents,
        subentries,
        linknode: cs_id,
    };
    let id = store_blob(ctx, blobstore, node).await?;
    Ok(HistoryManifestEntry::DeletedNode(id))
}

/// Collect and merge subentries from parent entries.
async fn collect_parent_subentries(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parents: &[HistoryManifestEntry],
) -> Result<ShardedMapV2Node<HistoryManifestEntry>> {
    let subentry_nodes: Vec<ShardedMapV2Node<HistoryManifestEntry>> =
        future::try_join_all(parents.iter().map(|parent| {
            Box::pin(async move {
                let subentries = match parent {
                    HistoryManifestEntry::File(id) => {
                        load_blob(ctx, blobstore, *id).await?.subentries
                    }
                    HistoryManifestEntry::DeletedNode(id) => {
                        load_blob(ctx, blobstore, *id).await?.subentries
                    }
                    HistoryManifestEntry::Directory(id) => {
                        load_blob(ctx, blobstore, *id).await?.subentries
                    }
                };
                Ok(subentries)
            }) as futures::future::BoxFuture<'_, Result<_>>
        }))
        .await?;

    // Filter out empty nodes to enable fast paths.
    let subentry_nodes: Vec<_> = subentry_nodes
        .into_iter()
        .filter(|node| node.size() > 0)
        .collect();

    if subentry_nodes.is_empty() {
        return Ok(Default::default());
    }

    // Fast path: single parent with subentries, reuse directly.
    if subentry_nodes.len() == 1 {
        return Ok(subentry_nodes.into_iter().next().unwrap());
    }

    // Multiple parents: stream entries one node at a time to avoid
    // materializing all entries simultaneously, and keep the first
    // occurrence of each key.
    let mut merged = BTreeMap::new();
    for node in subentry_nodes {
        let mut stream = std::pin::pin!(node.into_entries(ctx, blobstore));
        while let Some((key, entry)) = stream.try_next().await? {
            merged.entry(key).or_insert(entry);
        }
    }

    let trie_map: TrieMap<
        Either<HistoryManifestEntry, LoadableShardedMapV2Node<HistoryManifestEntry>>,
    > = merged
        .into_iter()
        .map(|(key, entry)| (key, Either::Left(entry)))
        .collect();

    ShardedMapV2Node::from_entries_and_partial_maps(ctx, blobstore, trie_map).await
}

/// Check if a parent has a file entry with matching content_id and file_type.
/// If found, return that parent's entry for reuse. This avoids creating
/// unnecessary merge nodes in history when a file was only changed on one
/// branch of a merge.
async fn find_reusable_file_entry_by_content(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parent_entries: &[(ChangesetId, HistoryManifestEntry)],
    content_id: ContentId,
    file_type: FileType,
) -> Result<Option<HistoryManifestEntry>> {
    for (_, entry) in parent_entries {
        if let HistoryManifestEntry::File(id) = entry {
            let file = load_blob(ctx, blobstore, *id).await?;
            if file.content_id == content_id && file.file_type == file_type {
                return Ok(Some(entry.clone()));
            }
        }
    }
    Ok(None)
}

/// Create implicit deletion unfold nodes from parent entries.
///
/// When a file replaces a directory, the old directory's children need
/// `DeletedNode` entries. This function takes the parent
/// entries at a path, finds any that are directories, and enumerates their
/// children as implicit deletion unfold nodes for the bounded traversal to
/// recurse into.
async fn create_implicit_deletion_unfold_nodes(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parent_entries: &[(ChangesetId, HistoryManifestEntry)],
    path: &MPath,
) -> Result<Vec<UnfoldNode>> {
    future::try_join_all(parent_entries.iter().map(|(parent_cs_id, parent_entry)| {
        let parent_cs_id = *parent_cs_id;
        let parent_entry = parent_entry.clone();
        async move {
            let dir_id = match get_directory_id(&parent_entry) {
                Some(id) => id,
                None => return anyhow::Ok(vec![]),
            };
            let dir: HistoryManifestDirectory = dir_id.load(ctx, blobstore).await?;
            let dir_children: Vec<(MPathElement, HistoryManifestEntry)> =
                dir.into_subentries(ctx, blobstore).try_collect().await?;

            let mut nodes = Vec::new();
            for (child_name, child_entry) in dir_children {
                let child_path = path.join(&child_name);
                let child_parent_dirs = if let Some(child_dir_id) = get_directory_id(&child_entry) {
                    let child_dir: HistoryManifestDirectory =
                        child_dir_id.load(ctx, blobstore).await?;
                    vec![(parent_cs_id, child_dir)]
                } else {
                    vec![]
                };
                nodes.push(UnfoldNode {
                    path_element: Some(child_name.clone()),
                    path: child_path,
                    change: None,
                    children: vec![],
                    parent_dirs: child_parent_dirs,
                    parent_entries: vec![(parent_cs_id, child_entry)],
                    implicit_deletion: true,
                });
            }
            Ok(nodes)
        }
    }))
    .await
    .map(|vecs| vecs.into_iter().flatten().collect())
}

/// Check whether all entries across all sources are deleted.
///
/// A directory is considered "all deleted" when it has at least one entry
/// and every entry is a `DeletedNode`.
async fn check_all_deleted(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    changed_entries: &[(Option<MPathElement>, HistoryManifestEntry)],
    reused: &[(
        SmallVec<[u8; 24]>,
        Either<HistoryManifestEntry, LoadableShardedMapV2Node<HistoryManifestEntry>>,
    )],
) -> Result<bool> {
    if changed_entries.is_empty() && reused.is_empty() {
        return Ok(false);
    }

    // Check individual entries first (cheap).
    if changed_entries.iter().any(|(_, e)| !is_deleted_entry(e)) {
        return Ok(false);
    }

    // A reused partial map may contain zero entries — for example when the
    // single parent's subtree is itself empty. An empty directory is not
    // "all deleted", so require at least one observed entry to return true.
    let mut saw_entry = !changed_entries.is_empty();

    for (_, reused_item) in reused {
        match reused_item {
            Either::Left(entry) => {
                if !is_deleted_entry(entry) {
                    return Ok(false);
                }
                saw_entry = true;
            }
            Either::Right(partial_map) => {
                let node = partial_map.clone().load(ctx, blobstore).await?;
                let mut stream = Box::pin(node.into_entries(ctx, blobstore));
                while let Some(result) = stream.next().await {
                    let (_, entry) = result?;
                    if !is_deleted_entry(&entry) {
                        return Ok(false);
                    }
                    saw_entry = true;
                }
            }
        }
    }

    Ok(saw_entry)
}

/// Unfold: decide what to do at each path and return children to recurse into.
async fn do_unfold(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    _cs_id: ChangesetId,
    node: UnfoldNode,
    replacement_paths: &BTreeSet<MPath>,
) -> Result<((Option<MPathElement>, UnfoldAction, MPath), Vec<UnfoldNode>)> {
    let UnfoldNode {
        path_element,
        path,
        change,
        children,
        parent_dirs,
        parent_entries,
        implicit_deletion,
    } = node;

    let has_children = !children.is_empty();
    let node_parents: Vec<HistoryManifestEntry> =
        parent_entries.iter().map(|(_, e)| e.clone()).collect();

    // Case 1: File created or modified.
    if let Some(Some((content_id, file_type))) = change {
        // Check if any parent had a directory here (file replaces directory).
        // If so, enumerate the old directory's children as implicit deletion
        // nodes. Their fold results will be collected into the file's
        // subentries by the CreateFile fold arm.
        let implicit_deletions =
            create_implicit_deletion_unfold_nodes(ctx, blobstore, &parent_entries, &path).await?;

        // Merge reuse: if the file content matches a parent's entry exactly,
        // reuse that parent's entry instead of creating a new one. This
        // avoids polluting history with merge nodes for files that were only
        // changed on one branch. Skip when there are implicit deletions
        // (file-replaces-directory needs a new entry with subentries).
        if implicit_deletions.is_empty() && parent_entries.len() > 1 {
            if let Some(reused) = find_reusable_file_entry_by_content(
                ctx,
                blobstore,
                &parent_entries,
                content_id,
                file_type,
            )
            .await?
            {
                let action = UnfoldAction::Reuse(reused);
                return Ok(((path_element, action, path), vec![]));
            }
        }

        let action = UnfoldAction::CreateFile {
            content_id,
            file_type,
            parents: node_parents,
        };
        return Ok(((path_element, action, path), implicit_deletions));
    }

    // Case 2: File deletion.
    // Either the file was explicitly deleted in the changeset, or we're
    // implicitly deleting a leaf file entry within a directory that has been
    // replaced with a file. Directory entries with implicit_deletion fall
    // through to Case 4, which recurses into their children.
    let bonsai_deletion = matches!(change, Some(None));
    let implicit_file_deletion =
        implicit_deletion && parent_entries.iter().all(|(_, e)| is_not_directory(e));
    if bonsai_deletion || implicit_file_deletion {
        // Check whether this has already been deleted in a parent, and if so
        // reuse the existing deletion node.
        if !parent_entries.is_empty() {
            // For merge commits, check if the node is the same across all
            // parents (note that in the single-parent case this will also be
            // true).
            let all_same = parent_entries.windows(2).all(|w| w[0].1 == w[1].1);
            if all_same && is_deleted_entry(&parent_entries[0].1) {
                let action = UnfoldAction::Reuse(parent_entries[0].1.clone());
                return Ok(((path_element, action, path), vec![]));
            }
        }

        let action = UnfoldAction::CreateDeletedFile {
            parents: node_parents,
        };
        return Ok(((path_element, action, path), vec![]));
    }

    // Case 3: A file entry with no bonsai changes that is also not being
    // implicitly deleted. This must be merge resolution where the outcome
    // of the resolution matches at least one of the parents.
    //
    // This case only applies when all parents have file entries (File or
    // DeletedNode). If any parent has a directory, we must fall through
    // to Case 5 (directory recursion) so merge_subtrees can combine the
    // contents from all parents.
    if !has_children
        && !parent_entries.is_empty()
        && parent_entries.iter().all(|(_, e)| is_not_directory(e))
    {
        if parent_entries.len() <= 1 {
            return Err(HistoryManifestDerivationError::InconsistentMerge.into());
        }

        let all_same = parent_entries.windows(2).all(|w| w[0].1 == w[1].1);
        if all_same {
            // All parents agree → reuse.
            let action = UnfoldAction::Reuse(parent_entries[0].1.clone());
            return Ok(((path_element, action, path), vec![]));
        }

        // Parents disagree on a file entry. No bonsai change means the
        // merge result matches the first parent — reuse it if it's a
        // live file.
        if let HistoryManifestEntry::File(_) = &parent_entries[0].1 {
            let action = UnfoldAction::Reuse(parent_entries[0].1.clone());
            return Ok(((path_element, action, path), vec![]));
        }

        // First parent is a DeletedNode — create a merge node.
        let all_deleted = parent_entries.iter().all(|(_, e)| is_deleted_entry(e));
        let action = if all_deleted {
            UnfoldAction::MergeDeletedFile {
                parents: node_parents,
            }
        } else {
            UnfoldAction::MergeFile {
                parents: node_parents,
            }
        };
        return Ok(((path_element, action, path), vec![]));
    }

    // Case 4: Directory recursion for an implicit deletion - we've deleted
    // a directory somewhere further up the tree, and now we need to recurse
    // so that we create deletion nodes for its children.
    if implicit_deletion {
        // All entries in this directory need deletion nodes. Enumerate
        // all parent entries as implicit deletion children.
        let deletion_children =
            create_implicit_deletion_unfold_nodes(ctx, blobstore, &parent_entries, &path).await?;

        let action = UnfoldAction::RecurseDirectory {
            parents: node_parents,
            reused: vec![],
        };
        return Ok(((path_element, action, path), deletion_children));
    }

    // Case 5: Recursion for a directory that has changes in the bonsai changeset.
    // Build the unfold node for children.
    let mut recurse_children: BTreeMap<MPathElement, UnfoldNode> = children
        .into_iter()
        .map(|(child_name, child_tree)| {
            let child_path = path.join(&child_name);
            let (child_change, grandchildren) = child_tree.deconstruct();

            // Note that this starts with empty parent info fields - we populate
            // these below for nodes that have entries in the parent(s).
            let node = UnfoldNode {
                path_element: Some(child_name.clone()),
                path: child_path,
                change: child_change,
                children: grandchildren,
                parent_dirs: vec![],
                parent_entries: vec![],
                implicit_deletion: false,
            };
            (child_name, node)
        })
        .collect();

    let merge_result =
        merge_subtrees(ctx, blobstore, &parent_dirs, recurse_children.keys()).await?;

    // Attach parent entries found during the traversal to the changed names.
    // For children whose path is itself a replacement target, leave the
    // parent info empty so the subtree is rebuilt from scratch.
    for (name, parent_entries) in merge_result.changed_parent_entries {
        if let Some(child_node) = recurse_children.get_mut(&name) {
            if replacement_paths.contains(&child_node.path) {
                continue;
            }
            for (parent_cs_id, entry) in &parent_entries {
                if let Some(dir_id) = get_directory_id(entry) {
                    let dir = load_blob(ctx, blobstore, dir_id).await?;
                    child_node.parent_dirs.push((*parent_cs_id, dir));
                }
                child_node
                    .parent_entries
                    .push((*parent_cs_id, entry.clone()));
            }
        }
    }

    // Add entries where parents disagree (not in our PathTree).
    // Construct UnfoldNodes from the raw disagreement info.
    for (name, parent_entries) in merge_result.disagreements {
        let child_path = path.join(&name);
        let mut child_parent_dirs = Vec::new();
        for (parent_cs_id, entry) in &parent_entries {
            if let Some(dir_id) = get_directory_id(entry) {
                let dir = load_blob(ctx, blobstore, dir_id).await?;
                child_parent_dirs.push((*parent_cs_id, dir));
            }
        }
        recurse_children.insert(
            name.clone(),
            UnfoldNode {
                path_element: Some(name),
                path: child_path,
                change: None,
                children: vec![],
                parent_dirs: child_parent_dirs,
                parent_entries,
                implicit_deletion: false,
            },
        );
    }

    let action = UnfoldAction::RecurseDirectory {
        parents: node_parents,
        reused: merge_result.reused,
    };
    Ok((
        (path_element, action, path),
        recurse_children.into_values().collect(),
    ))
}

/// Fold: construct the actual node from the unfold's decision and child results.
async fn do_fold(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    cs_id: ChangesetId,
    path: &MPath,
    action: UnfoldAction,
    subentries_iter: impl IntoIterator<Item = (Option<MPathElement>, HistoryManifestEntry)>,
) -> Result<HistoryManifestEntry> {
    match action {
        UnfoldAction::Reuse(entry) => Ok(entry),

        UnfoldAction::CreateFile {
            content_id,
            file_type,
            parents,
        } => {
            // Collect child fold results (from implicit deletions when a
            // file replaces a directory) and parent subentries (from prior
            // file-replaces-directory events). Both must be merged.
            let fold_results: Vec<_> = subentries_iter.into_iter().collect();
            let parent_subentries = collect_parent_subentries(ctx, blobstore, &parents).await?;

            let subentries = if fold_results.is_empty() {
                parent_subentries
            } else {
                // Merge fold results with parent subentries. Fold results
                // (from the current commit) take precedence.
                let mut parent_entries: BTreeMap<SmallVec<[u8; 24]>, HistoryManifestEntry> =
                    parent_subentries
                        .into_entries(ctx, blobstore)
                        .try_collect()
                        .await?;
                for (maybe_name, entry) in fold_results {
                    let name = maybe_name.expect("File subentry must have a path element");
                    parent_entries.insert(name.to_smallvec(), entry);
                }
                let trie_map: TrieMap<
                    Either<HistoryManifestEntry, LoadableShardedMapV2Node<HistoryManifestEntry>>,
                > = parent_entries
                    .into_iter()
                    .map(|(key, entry)| (key, Either::Left(entry)))
                    .collect();
                ShardedMapV2Node::from_entries_and_partial_maps(ctx, blobstore, trie_map).await?
            };

            let file = HistoryManifestFile {
                parents,
                content_id,
                file_type,
                path_hash: path.get_path_hash(),
                linknode: cs_id,
                subentries,
            };
            let id = store_blob(ctx, blobstore, file).await?;
            Ok(HistoryManifestEntry::File(id))
        }

        UnfoldAction::CreateDeletedFile { parents } => {
            let subentries = collect_parent_subentries(ctx, blobstore, &parents).await?;
            let node = HistoryManifestDeletedNode {
                parents,
                subentries,
                linknode: cs_id,
            };
            let id = store_blob(ctx, blobstore, node).await?;
            Ok(HistoryManifestEntry::DeletedNode(id))
        }

        UnfoldAction::MergeFile { parents } => {
            merge_live_file_node(ctx, blobstore, cs_id, path, parents).await
        }

        UnfoldAction::MergeDeletedFile { parents } => {
            merge_deleted_node(ctx, blobstore, cs_id, path, parents).await
        }

        UnfoldAction::RecurseDirectory { parents, reused } => {
            // Collect changed entries from child fold results.
            let changed_entries: Vec<_> = subentries_iter.into_iter().collect();

            let is_deleted = check_all_deleted(ctx, blobstore, &changed_entries, &reused).await?;

            // Build trie from changed entries and reused entries/subtrees.
            let trie_map: TrieMap<
                Either<HistoryManifestEntry, LoadableShardedMapV2Node<HistoryManifestEntry>>,
            > = changed_entries
                .into_iter()
                .map(|(maybe_name, entry)| {
                    let name = maybe_name.expect("Directory subentry must have a path element");
                    (name.to_smallvec(), Either::Left(entry))
                })
                .chain(reused)
                .collect();

            let subentries =
                ShardedMapV2Node::from_entries_and_partial_maps(ctx, blobstore, trie_map).await?;

            // The root of a history manifest must always be a Directory, even
            // when every entry under it is deleted (e.g., a commit that
            // removes the only remaining file). This mirrors the unode
            // manifest, which synthesizes an empty root manifest in the same
            // situation.
            if is_deleted && !path.is_root() {
                let node = HistoryManifestDeletedNode {
                    parents,
                    subentries,
                    linknode: cs_id,
                };
                let id = store_blob(ctx, blobstore, node).await?;
                Ok(HistoryManifestEntry::DeletedNode(id))
            } else {
                let dir = HistoryManifestDirectory {
                    parents,
                    subentries,
                    linknode: cs_id,
                };
                let id = store_blob(ctx, blobstore, dir).await?;
                Ok(HistoryManifestEntry::Directory(id))
            }
        }
    }
}

/// Process subtree copies from a bonsai changeset.
///
/// Each subtree copy from `(from_cs_id, from_path)` to `to_path` becomes:
/// - A "replacement path" at `to_path`, whose parent entries are cleared
///   during the traversal so the destination subtree is rebuilt from
///   scratch.
/// - Synthetic file changes enumerating every live file in the source
///   directory (or the source file itself), excluding paths already
///   changed in this commit or covered by a nested subtree copy.
///
/// History manifest entries are unique to each commit (their `linknode`
/// records which commit they belong to), so the source entries cannot be
/// reused directly; every file under the source must be re-materialized
/// at the destination. The cost of the copy is therefore linear in the
/// size of the source subtree.
async fn process_subtree_copies(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, RootHistoryManifestDirectoryId>>,
    bonsai: &BonsaiChangeset,
    file_changes: &[(NonRootMPath, Option<(ContentId, FileType)>)],
) -> Result<(
    Vec<(NonRootMPath, Option<(ContentId, FileType)>)>,
    BTreeSet<MPath>,
)> {
    let blobstore = derivation_ctx.blobstore();
    let subtree_changes = bonsai.subtree_changes();

    let mut replacement_paths: BTreeSet<MPath> = BTreeSet::new();
    let mut additional_changes: Vec<(NonRootMPath, Option<(ContentId, FileType)>)> = Vec::new();

    for (to_path, subtree_change) in subtree_changes.iter() {
        let Some((from_cs_id, from_path)) = subtree_change.copy_source() else {
            continue;
        };
        let from_root = derivation_ctx
            .fetch_unknown_dependency::<RootHistoryManifestDirectoryId>(ctx, known, from_cs_id)
            .await?
            .into_history_manifest_directory_id();
        let from_entry = from_root
            .find_entry(ctx.clone(), blobstore.clone(), from_path.clone())
            .await
            .with_context(|| {
                format!("Failed to fetch subtree copy source {from_cs_id}:{from_path}")
            })?
            .ok_or_else(|| format_err!("No subtree copy source {from_cs_id}:{from_path}"))?;

        replacement_paths.insert(to_path.clone());

        match from_entry {
            Entry::Tree(from_dir_id) => {
                // Build the set of paths (relative to to_path) that are
                // either changed in the current commit or covered by a
                // nested subtree copy. These should not be synthesized.
                let mut changed_paths: PathTree<bool> = file_changes
                    .iter()
                    .filter_map(|(change_path, _)| {
                        let change_mpath: &MPath = change_path.into();
                        if to_path.is_prefix_of(change_mpath) {
                            Some((change_mpath.remove_prefix_component(to_path), true))
                        } else {
                            None
                        }
                    })
                    .collect();
                for (other_to_path, other_subtree_change) in subtree_changes.iter() {
                    if other_to_path != to_path
                        && other_subtree_change.copy_source().is_some()
                        && to_path.is_prefix_of(other_to_path)
                    {
                        let subpath = other_to_path.remove_prefix_component(to_path);
                        changed_paths.insert(subpath, true);
                    }
                }

                let changed_paths = Arc::new(changed_paths);
                let filter_changed_paths =
                    move |path: &MPath| changed_paths.get(path).is_none_or(|x| !x);
                from_dir_id
                    .find_entries_filtered(
                        ctx.clone(),
                        blobstore.clone(),
                        Some(PathOrPrefix::Prefix(MPath::ROOT)),
                        {
                            cloned!(filter_changed_paths);
                            move |path, _mf_id| filter_changed_paths(path)
                        },
                    )
                    .map_ok(|(path, entry)| {
                        let include = filter_changed_paths(&path);
                        async move {
                            match entry {
                                Entry::Leaf(file_id) if include => {
                                    let file = load_blob(ctx, blobstore, file_id).await?;
                                    Ok(Some((
                                        to_path.join(&path),
                                        Some((file.content_id, file.file_type)),
                                    )))
                                }
                                _ => Ok(None),
                            }
                        }
                    })
                    .try_buffered(100)
                    .try_for_each(|change| {
                        if let Some((path, change)) = change {
                            if let Some(path) = path.into_optional_non_root_path() {
                                additional_changes.push((path, change));
                            }
                        }
                        future::ready(Ok(()))
                    })
                    .await?;
            }
            Entry::Leaf(from_file_id) => {
                // Only synthesize if the destination is not already
                // changed in this commit.
                let already_changed = file_changes.iter().any(|(change_path, _)| {
                    let change_mpath: &MPath = change_path.into();
                    change_mpath == to_path
                });
                if !already_changed {
                    let from_file = load_blob(ctx, blobstore, from_file_id).await?;
                    let to_non_root =
                        to_path
                            .clone()
                            .into_optional_non_root_path()
                            .ok_or_else(|| {
                                format_err!("Subtree copy for root cannot copy from a file")
                            })?;
                    additional_changes.push((
                        to_non_root,
                        Some((from_file.content_id, from_file.file_type)),
                    ));
                }
            }
        }
    }

    // Every replacement path must end up with at least one file entry
    // once all subtree copies and the bonsai's own file changes are
    // considered. Otherwise the destination would have no content in the
    // combined PathTree, and `merge_subtrees` would silently carry over
    // the parent's content at that path via its `reused` output — giving
    // the subtree copy no effect.
    for rp in &replacement_paths {
        let has_content = file_changes
            .iter()
            .chain(additional_changes.iter())
            .any(|(p, _)| {
                let p: &MPath = p.into();
                rp.is_prefix_of(p) || p == rp
            });
        if !has_content {
            return Err(format_err!(
                "Subtree copy destination {rp} has no content after processing: the copy \
                 source has no live entries and no file changes cover the destination"
            ));
        }
    }

    Ok((additional_changes, replacement_paths))
}

/// Derive the history manifest entry at an arbitrary subtree path.
///
/// Derives only the subtree rooted at `prefix`. Parent entries are at the
/// stage level (the entry at `prefix` in each parent commit, not the root).
///
/// `known_entries` maps absolute paths to pre-computed entries from
/// dependency stages — used to short-circuit recursion.
pub(crate) async fn derive_history_manifest_entry(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    bonsai: &BonsaiChangeset,
    parent_entries: Vec<(ChangesetId, HistoryManifestEntry)>,
    prefix: MPath,
    known_entries: HashMap<MPath, Option<HistoryManifestEntry>>,
) -> Result<Option<HistoryManifestEntry>> {
    let blobstore = derivation_ctx.blobstore();

    // 1. Collect ALL file changes (unfiltered) — process_subtree_copies
    //    needs the full set to correctly detect overlaps and replacements.
    let all_file_changes: Vec<(NonRootMPath, Option<(ContentId, FileType)>)> = bonsai
        .file_changes()
        .map(|(path, change)| {
            (
                path.clone(),
                match change {
                    FileChange::Change(tc) => Some((tc.content_id(), tc.file_type())),
                    FileChange::UntrackedChange(uc) => Some((uc.content_id(), uc.file_type())),
                    FileChange::Deletion | FileChange::UntrackedDeletion => None,
                },
            )
        })
        .collect();

    // 2. Process subtree copies with the full file changes, then scope
    //    everything to the prefix.
    let (additional_changes, replacement_paths) =
        process_subtree_copies(ctx, derivation_ctx, None, bonsai, &all_file_changes).await?;

    let mut file_changes: Vec<_> = all_file_changes
        .into_iter()
        .filter(|(path, _)| prefix.is_prefix_of(path.as_mpath()))
        .collect();
    let additional_changes: Vec<_> = additional_changes
        .into_iter()
        .filter(|(path, _)| prefix.is_prefix_of(path.as_mpath()))
        .collect();
    file_changes.extend(additional_changes);

    let replacement_paths: BTreeSet<MPath> = replacement_paths
        .into_iter()
        .filter(|path| prefix.is_prefix_of(path) || path.is_prefix_of(&prefix))
        .collect();

    // 3. Build PathTree from scoped file changes and navigate to prefix.
    let changes: PathTree<Option<Option<(ContentId, FileType)>>> = PathTree::from_iter(
        file_changes
            .into_iter()
            .map(|(path, change)| (path, Some(change))),
    );

    let subtree = {
        let mut tree = changes;
        for elem in prefix.clone().into_iter() {
            let (_, children) = tree.deconstruct();
            tree = children
                .into_iter()
                .find(|(name, _)| name == &elem)
                .map(|(_, child)| child)
                .unwrap_or_default();
        }
        tree
    };

    // 4. Load parent directories from stage-level parent entries.
    let parent_dirs: Vec<(ChangesetId, HistoryManifestDirectory)> = future::try_join_all(
        parent_entries
            .iter()
            .filter_map(|(cs_id, entry)| match entry {
                HistoryManifestEntry::Directory(dir_id) => {
                    let cs_id = *cs_id;
                    let dir_id = *dir_id;
                    Some(async move {
                        let dir: HistoryManifestDirectory = dir_id.load(ctx, blobstore).await?;
                        anyhow::Ok((cs_id, dir))
                    })
                }
                _ => None,
            }),
    )
    .await?;

    let (change, children) = subtree.deconstruct();
    let has_changes = change.is_some() || !children.is_empty();

    // If the prefix (or an ancestor) is a subtree-copy destination, the
    // subtree is rebuilt from scratch: the parent entries no longer apply, so
    // clear them before any parent-based short-circuit. Without this, a stage
    // under a copy destination that the copy removed (e.g. `top1/sub` when
    // `top1` is replaced by a `top2` that has no `sub`) would wrongly reuse
    // the pre-copy parent entry instead of resolving to "removed".
    let is_replacement = replacement_paths
        .iter()
        .any(|rp| rp.is_prefix_of(&prefix) || *rp == prefix);
    let (parent_dirs, parent_entries) = if is_replacement {
        (vec![], vec![])
    } else {
        (parent_dirs, parent_entries)
    };

    // 5. No changes, no parents, no live known entries at this prefix → doesn't exist.
    //    known_entries with all-None values (dependency stages where the path
    //    doesn't exist) are treated as empty.
    let has_live_known = known_entries.values().any(|v| v.is_some());
    if !has_changes && parent_entries.is_empty() && !has_live_known && !prefix.is_root() {
        return Ok(None);
    }

    // 6. No changes, no live known entries, all parents agree → reuse parent entry.
    if !has_changes && !has_live_known && !prefix.is_root() && !parent_entries.is_empty() {
        let all_same = parent_entries.windows(2).all(|w| w[0].1 == w[1].1);
        if all_same {
            return Ok(Some(parent_entries[0].1.clone()));
        }
    }

    // 7. Keep the full known-entry map, including paths a dependency stage
    //    computed as empty (`None`). A `Some` entry short-circuits the
    //    traversal by reuse; a `None` entry means the path is empty, so we
    //    drop it from its parent's children rather than recursing into it
    //    again.
    let known_entry_map: HashMap<MPath, Option<HistoryManifestEntry>> = known_entries;

    // 8. Run bounded_traversal from the prefix node. `parent_dirs` /
    //    `parent_entries` were already cleared above if this is a replacement.
    let root_node = UnfoldNode {
        path_element: prefix.basename().cloned(),
        path: prefix.clone(),
        change,
        children,
        parent_dirs: parent_dirs.clone(),
        parent_entries: parent_entries.clone(),
        implicit_deletion: false,
    };

    let ctx_ref = ctx;
    cloned!(ctx, blobstore);
    let traversal_handle = mononoke::spawn_task(async move {
        borrowed!(ctx, blobstore, replacement_paths, known_entry_map);

        let result: (Option<MPathElement>, HistoryManifestEntry) = bounded_traversal(
            256,
            root_node,
            // unfold: short-circuit at known entry paths
            {
                move |node: UnfoldNode| {
                    async move {
                        // A dependency stage already derived this exact path;
                        // reuse its entry instead of recursing.
                        if let Some(Some(entry)) = known_entry_map.get(&node.path) {
                            let action = UnfoldAction::Reuse(entry.clone());
                            return Ok(((node.path_element, action, node.path), vec![]));
                        }
                        let (info, children) =
                            do_unfold(ctx, blobstore, cs_id, node, replacement_paths).await?;
                        // Drop children a dependency stage computed as empty
                        // (`None`) so we neither recurse into them nor add
                        // them to this directory's subentries.
                        let children: Vec<UnfoldNode> = children
                            .into_iter()
                            .filter(|child| !matches!(known_entry_map.get(&child.path), Some(None)))
                            .collect();
                        anyhow::Ok((info, children))
                    }
                    .boxed()
                }
            },
            // fold
            move |(path_element, action, path): (Option<MPathElement>, UnfoldAction, MPath),
                  subentries_iter| {
                async move {
                    let result =
                        do_fold(ctx, blobstore, cs_id, &path, action, subentries_iter).await?;
                    Ok((path_element, result))
                }
                .boxed()
            },
        )
        .await?;

        let (_path_element, entry) = result;
        anyhow::Ok(entry)
    });

    let entry = traversal_handle.await??;

    // Post-traversal reuse: if the traversal produced a directory whose
    // subentries match a single parent's subentries exactly, the subtree
    // is effectively unchanged — reuse the parent entry. This matches
    // canonical derivation where merge_subtrees at the root level
    // detects unchanged subtrees and puts them in `reused`.
    if !prefix.is_root() && parent_entries.len() == 1 {
        if let HistoryManifestEntry::Directory(new_dir_id) = &entry {
            if let (_, HistoryManifestEntry::Directory(parent_dir_id)) = &parent_entries[0] {
                let bs = derivation_ctx.blobstore();
                let new_dir: HistoryManifestDirectory = new_dir_id.load(ctx_ref, bs).await?;
                let parent_dir: HistoryManifestDirectory = parent_dir_id.load(ctx_ref, bs).await?;
                if new_dir.subentries == parent_dir.subentries {
                    return Ok(Some(parent_entries[0].1.clone()));
                }
            }
        }
    }

    Ok(Some(entry))
}

pub(crate) async fn derive_history_manifest(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, RootHistoryManifestDirectoryId>>,
    cs_id: ChangesetId,
    bonsai: &BonsaiChangeset,
    parents: Vec<HistoryManifestDirectoryId>,
) -> Result<HistoryManifestDirectoryId> {
    let blobstore = derivation_ctx.blobstore();

    // 1. Collect file changes from the bonsai changeset.
    let mut file_changes: Vec<(NonRootMPath, Option<(ContentId, FileType)>)> = bonsai
        .file_changes()
        .map(|(path, change)| {
            (
                path.clone(),
                match change {
                    FileChange::Change(tc) => Some((tc.content_id(), tc.file_type())),
                    FileChange::UntrackedChange(uc) => Some((uc.content_id(), uc.file_type())),
                    FileChange::Deletion | FileChange::UntrackedDeletion => None,
                },
            )
        })
        .collect();

    // 2. Process subtree copies: synthesize additional file changes from
    //    each copy source and collect the set of destination paths whose
    //    parent entries will be replaced during the traversal.
    let (additional_changes, replacement_paths) =
        process_subtree_copies(ctx, derivation_ctx, known, bonsai, &file_changes).await?;
    file_changes.extend(additional_changes);

    // 3. Build PathTree from combined file changes.
    let changes: PathTree<Option<Option<(ContentId, FileType)>>> = PathTree::from_iter(
        file_changes
            .into_iter()
            .map(|(path, change)| (path, Some(change))),
    );

    // 4. Load parent root directories concurrently.
    let parent_dirs: Vec<(ChangesetId, HistoryManifestDirectory)> = {
        let parent_cs_ids: Vec<ChangesetId> = bonsai.parents().collect();
        future::try_join_all(parents.iter().zip(parent_cs_ids.iter()).map(
            |(dir_id, parent_cs_id)| async move {
                let dir: HistoryManifestDirectory = dir_id.load(ctx, blobstore).await?;
                anyhow::Ok((*parent_cs_id, dir))
            },
        ))
        .await?
    };

    // 5. Run bounded_traversal inside a spawned task.
    let root_node = {
        let (change, children) = changes.deconstruct();
        let parent_entries: Vec<(ChangesetId, HistoryManifestEntry)> = parent_dirs
            .iter()
            .map(|(cs_id, dir)| {
                // Root parent entries are Directory entries pointing to the root dir IDs.
                let dir_id = dir.get_directory_id();
                (*cs_id, HistoryManifestEntry::Directory(dir_id))
            })
            .collect();

        UnfoldNode {
            path_element: None,
            path: MPath::ROOT,
            change,
            children,
            parent_dirs: parent_dirs.clone(),
            parent_entries,
            implicit_deletion: false,
        }
    };

    cloned!(ctx, blobstore);
    let traversal_handle = mononoke::spawn_task(async move {
        borrowed!(ctx, blobstore, replacement_paths);

        let result = bounded_traversal(
            256,
            root_node,
            // unfold
            {
                move |node: UnfoldNode| {
                    async move { do_unfold(ctx, blobstore, cs_id, node, replacement_paths).await }
                        .boxed()
                }
            },
            // fold
            move |(path_element, action, path): (Option<MPathElement>, UnfoldAction, MPath),
                  subentries_iter| {
                async move {
                    let result =
                        do_fold(ctx, blobstore, cs_id, &path, action, subentries_iter).await?;
                    Ok((path_element, result))
                }
                .boxed()
            },
        )
        .await?;

        // Result is (None, entry) for root.
        let (_path_element, entry) = result;
        match entry {
            HistoryManifestEntry::Directory(id) => Ok(id),
            _ => Err(HistoryManifestDerivationError::InvalidRootDirectory.into()),
        }
    });

    traversal_handle.await?
}
