// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{Entry, Manifest, PathTree};
use blobstore::Blobstore;
use context::CoreContext;
use failure::{format_err, Error};
use futures::{future, Future, IntoFuture};
use futures_ext::{bounded_traversal::bounded_traversal, FutureExt};
use mononoke_types::{Loadable, MPath, MPathElement};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    hash::Hash,
    iter::FromIterator,
    mem,
};

/// Information passed to `create_tree` function when tree node is constructed
pub struct TreeInfo<TreeId, LeafId> {
    pub path: Option<MPath>,
    pub parents: Vec<TreeId>,
    pub subentries: BTreeMap<MPathElement, Entry<TreeId, LeafId>>,
}

/// Information passed to `create_leaf` function when leaf node is constructed
pub struct LeafInfo<LeafId, Leaf> {
    pub path: MPath,
    pub parents: Vec<LeafId>,
    /// Leaf value, if it is not provided it means we have leaves only conflict
    /// which can potentially be resolved by `create_leaf`, in case of mercurial
    /// multiple leaves with the same content can be successfully resolved.
    pub leaf: Option<Leaf>,
}

/// Construct a new manifest from parent manifests and a list of changes from a bonsai commit.
/// The manifest is constructed in accordance with bonsai semantic (see below for explanation).
///
/// Parent manifests should have been constructed for each parent of this bonsai commit
/// (note that there can be more than 2 parents).
///
/// In the context of this function "manifest" means a set of leaves and trees. Manifest is a
/// recursive structure that starts with a root tree which can point to other trees or leaves.
/// Each leaf represents a file in a repository, and each tree represents a directory.
///
/// Note that while `derive_manifest` can be used to construct Mercurial manifests
/// (e.g. Manifests where leafs are filenodes), it's not limited to them. Leafs can be arbitrary
/// ids e.g. SHA-1 content of a file, unode ids. Tree ids can also be arbitrary.
///
/// ## Bonsai Semantic
/// Bonsai semantic is a set of rules about how to apply changes to manifests. Each change is
/// either `Some(Leaf)` meaning that a new file exists in the new commit (either new file is
/// created or reused from one of the parents), `None` meaning that the file was deleted from
/// one of the parents and no longer exist in the new bonsai commit.
/// Also see: [Bonsai changeset actions](https://fb.quip.com/A2kqArd9Nb90)
///
/// Changes are applied recursively starting from the root of parent manifests.
/// Here is how changes affect conflict resolution
/// 1. If no change ends on the current path or any subpaths e.g. we are just merging parent
///    manifests.
///    - If all entries in parent manifests are identical (regardless of whether they are trees
///      or leaves) then we are just reusing the current entry.
///    - If all parents entries are trees then recurse into all of them and continue merging.
///    - If all parent entries are leaves then `create_leaf` is called with `None` leaf, this
///      can result in successful merge (for example, mercurial can merge different entries with
///      same content).
///    - If we have a mix of leaves/trees in parent entries - we have a broken commit and this is
///      an unresolved conflict error.
/// 2. If no change ends on the current path BUT there are changes on the subpaths (e.g. we are
///    on "A/", but there's "A/file.txt").
///    - If all parents entries are trees then recurse into all of them and continue merging
///    - If we have a single leaf in parent entries - we have a broken commit and this is an
///      unresolved conflict error.
/// 3. Current path have `None` change associated with it.
///   - Only trees: invalid changes.
///   - Only leaves: all leaves are removed.
///   - Mix of leaves/trees: all leaves are removed, recurse into the trees.
/// 4. Current path have `Some(leaf)` change associated with it.
///   - _: all the trees are removed in favour of this leaf.
///
pub fn derive_manifest<TreeId, LeafId, Leaf, T, TFut, L, LFut>(
    ctx: CoreContext,
    blobstore: impl Blobstore + Clone,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (MPath, Option<Leaf>)>,
    mut create_tree: T,
    mut create_leaf: L,
) -> impl Future<Item = Option<TreeId>, Error = Error>
where
    LeafId: Send + Copy + Eq + Hash + fmt::Debug + 'static,
    TreeId: Loadable + Send + Copy + Eq + Hash + fmt::Debug + 'static,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId>,
    T: FnMut(TreeInfo<TreeId, LeafId>) -> TFut,
    TFut: IntoFuture<Item = TreeId, Error = Error>,
    TFut::Future: Send + 'static,
    L: FnMut(LeafInfo<LeafId, Leaf>) -> LFut,
    LFut: IntoFuture<Item = LeafId, Error = Error>,
    LFut::Future: Send + 'static,
{
    bounded_traversal(
        256,
        MergeNode {
            name: None,
            path: None,
            changes: PathTree::from_iter(
                changes
                    .into_iter()
                    .map(|(path, change)| (path, Some(Change::from(change)))),
            ),
            parents: parents.into_iter().map(Entry::Tree).collect(),
        },
        // unfold, all merge logic happens in this unfold function
        move |merge_node| merge_node.merge(ctx.clone(), blobstore.clone()),
        // fold, this function only creates entries from merge result and already merged subentries
        move |merge_result, subentries| match merge_result {
            MergeResult::Reuse { name, entry } => future::ok(Some((name, entry))).boxify(),
            MergeResult::Delete => future::ok(None).boxify(),
            MergeResult::CreateTree {
                name,
                path,
                parents,
            } => {
                let subentries: BTreeMap<_, _> = subentries
                    .flatten()
                    .filter_map(|(name, entry): (Option<MPathElement>, _)| {
                        name.map(|name| (name, entry))
                    })
                    .collect();
                if subentries.is_empty() {
                    future::ok(None).boxify()
                } else {
                    create_tree(TreeInfo {
                        path,
                        parents,
                        subentries,
                    })
                    .into_future()
                    .map(move |tree_id| Some((name, Entry::Tree(tree_id))))
                    .boxify()
                }
            }
            MergeResult::CreateLeaf {
                leaf,
                name,
                path,
                parents,
            } => create_leaf(LeafInfo {
                leaf,
                path,
                parents,
            })
            .into_future()
            .map(move |leaf_id| Some((name, Entry::Leaf(leaf_id))))
            .boxify(),
        },
    )
    .map(|result: Option<_>| result.and_then(|(_, entry)| entry.into_tree()))
}

// Change is isomorphic to Option, but it makes it easier to understand merge logic
enum Change<LeafId> {
    Add(LeafId),
    Remove,
}

impl<Leaf> From<Option<Leaf>> for Change<Leaf> {
    fn from(change: Option<Leaf>) -> Self {
        change.map(Change::Add).unwrap_or(Change::Remove)
    }
}

enum MergeResult<TreeId, LeafId, Leaf> {
    Delete,
    Reuse {
        name: Option<MPathElement>,
        entry: Entry<TreeId, LeafId>,
    },
    CreateLeaf {
        leaf: Option<Leaf>,
        name: Option<MPathElement>,
        path: MPath,
        parents: Vec<LeafId>,
    },
    CreateTree {
        name: Option<MPathElement>,
        path: Option<MPath>,
        parents: Vec<TreeId>,
    },
}

/// This node represents unmerged state of `parents.len() + 1` way merge
/// between changes and parents.
struct MergeNode<TreeId, LeafId, Leaf> {
    name: Option<MPathElement>, // name of this node in parent manifest
    path: Option<MPath>,        // path to this node from root of the manifest
    changes: PathTree<Option<Change<Leaf>>>, // changes associated with current subtree
    parents: Vec<Entry<TreeId, LeafId>>, // unmerged parents of current node
}

impl<TreeId, LeafId, Leaf> MergeNode<TreeId, LeafId, Leaf>
where
    LeafId: Copy + Eq + Hash + fmt::Debug,
    TreeId: Loadable + Copy + Eq + Hash + fmt::Debug,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId>,
{
    fn merge(
        self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> impl Future<Item = (MergeResult<TreeId, LeafId, Leaf>, Vec<Self>), Error = Error> {
        let MergeNode {
            name,
            path,
            changes:
                PathTree {
                    value: change,
                    subentries,
                },
            mut parents,
        } = self;

        // Deduplicate entries in parents list, **preseriving order** of entries.
        // Essencially perfroming trivial merge between identical entries.
        {
            let mut visited = HashSet::new();
            parents.retain(|parent| visited.insert(*parent));
        }

        // Apply change
        // If we create `parnte_trees` (none of the return statement have been reached), this
        // indicates that file/tree conflict if any, has been resolved in favour of tree merge.
        let parent_subtrees = match change {
            None => match *parents {
                // Changes does not have entry associated with current path
                [parent_entry] => {
                    // Only one tree/leaf is left
                    if !subentries.is_empty() {
                        match parent_entry {
                            Entry::Leaf(_) => {
                                // Current entry is a leaf but we still have changes that needs
                                // to be applied to its subentries, we cannot resolve this merge.
                                let error = format_err!(
                                    "Can not apply changes to a leaf:\npath: {:?}\nparents: {:?}",
                                    path,
                                    parents
                                );
                                return future::err(error).left_future();
                            }
                            Entry::Tree(tree_id) => {
                                // We have tree entry, and changes that needs to be applied
                                // to its subentries, we cannot reuse this entry and have to recurse
                                vec![tree_id]
                            }
                        }
                    } else {
                        // We have single entry and do not have any changes associated with it subentries,
                        // it is safe to reuse current entry as is.
                        return future::ok((
                            MergeResult::Reuse {
                                name,
                                entry: parent_entry,
                            },
                            Vec::new(),
                        ))
                        .left_future();
                    }
                }
                _ => {
                    // Split entries int leaves and trees.
                    let mut leaves = Vec::new();
                    let mut trees = Vec::new();
                    for entry in parents.iter() {
                        match entry {
                            Entry::Leaf(leaf) => leaves.push(*leaf),
                            Entry::Tree(tree) => trees.push(*tree),
                        }
                    }

                    if leaves.is_empty() {
                        // We do not have any leaves at this point, and should proceed with
                        // merging of threes
                        trees
                    } else if trees.is_empty() && subentries.is_empty() {
                        // We have leaves only but their ids are not equal to each other,
                        // this should immediately indicate conflict, as mercurial can successfully
                        // merge this leaves if they have identical content.
                        return future::ok((
                            MergeResult::CreateLeaf {
                                leaf: None,
                                name,
                                path: path.expect("leaf can not have empty path"),
                                parents: leaves,
                            },
                            Vec::new(),
                        ))
                        .left_future();
                    } else {
                        // We can get here in two cases:
                        //   - we have mix of trees and leaves.
                        //   - all of the parents are leaves, but we have changes that need to be
                        //     applied to it current nodes subentries.
                        // both of this situation result in unresolvable conflict.
                        let error = format_err!(
                            "Unresolved conflict at:\npath: {:?}\nparents: {:?}",
                            path,
                            parents
                        );
                        return future::err(error).left_future();
                    }
                }
            },
            Some(Change::Remove) => {
                // Remove associated Leaf entr{y|ies}, leaving only trees.
                // This case is used to either remove leaf entry or resolve file/tree conflict
                // in favour of tree merge.
                parents.into_iter().filter_map(Entry::into_tree).collect()
            }
            Some(Change::Add(leaf)) => {
                // Replace current merge node with a leaf, and stop traversal.
                // This case is used ot either replace leaf entry or resolve file/tree conflict
                // in favour or file.
                return future::ok((
                    MergeResult::CreateLeaf {
                        leaf: Some(leaf),
                        name,
                        path: path.expect("leaf can not have empty path"),
                        parents: parents.into_iter().filter_map(Entry::into_leaf).collect(),
                    },
                    Vec::new(),
                ))
                .left_future();
            }
        };

        if parent_subtrees.is_empty() && subentries.is_empty() {
            // All elements of this merge tree have been deleted.
            // Nothing left to do apart from inidicating that this node needs to be removed
            // from its parent.
            return future::ok((MergeResult::Delete, Vec::new())).left_future();
        }

        // Fetch parent trees and merge them.
        future::join_all(
            parent_subtrees
                .iter()
                .map(move |tree_id| tree_id.load(ctx.clone(), &blobstore))
                .collect::<Vec<_>>(),
        )
        .map(move |manifests| {
            let mut deps: BTreeMap<MPathElement, Self> = Default::default();
            // add subentries from all parents
            for manifest in manifests {
                for (name, entry) in manifest.list() {
                    let subentry = deps.entry(name.clone()).or_insert_with(|| MergeNode {
                        path: Some(MPath::join_opt_element(path.as_ref(), &name)),
                        name: Some(name),
                        changes: Default::default(),
                        parents: Default::default(),
                    });
                    subentry.parents.push(entry);
                }
            }
            // add subentries from changes
            for (name, change) in subentries {
                let subentry = deps.entry(name.clone()).or_insert_with(|| MergeNode {
                    path: Some(MPath::join_opt_element(path.as_ref(), &name)),
                    name: Some(name),
                    changes: Default::default(),
                    parents: Default::default(),
                });
                mem::replace(&mut subentry.changes, change);
            }

            (
                MergeResult::CreateTree {
                    name,
                    path,
                    parents: parent_subtrees,
                },
                deps.into_iter().map(|(_name, dep)| dep).collect(),
            )
        })
        .right_future()
    }
}
