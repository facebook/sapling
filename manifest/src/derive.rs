/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{Entry, Manifest, PathTree, StoreLoadable};
use anyhow::{format_err, Error};
use context::CoreContext;
use futures::{future, Future, IntoFuture};
use futures_ext::{bounded_traversal::bounded_traversal, FutureExt};
use mononoke_types::{MPath, MPathElement};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    hash::Hash,
    iter::FromIterator,
    mem,
};
use tracing::{trace_args, EventId, Traced};

/// Information passed to `create_tree` function when tree node is constructed
///
/// `Ctx` is any additional data which is useful for particular implementation of
/// manifest. It is `Some` for subentries for which `create_{tree|leaf}` was called to
/// generate them, and `None` if subentry was reused from one of its parents.
pub struct TreeInfo<TreeId, LeafId, Ctx> {
    pub path: Option<MPath>,
    pub parents: Vec<TreeId>,
    pub subentries: BTreeMap<MPathElement, (Option<Ctx>, Entry<TreeId, LeafId>)>,
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
pub fn derive_manifest<TreeId, LeafId, Leaf, T, TFut, L, LFut, Ctx, Store>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (MPath, Option<Leaf>)>,
    mut create_tree: T,
    mut create_leaf: L,
) -> impl Future<Item = Option<TreeId>, Error = Error>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    TreeId: StoreLoadable<Store> + Send + Clone + Eq + Hash + fmt::Debug + 'static,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId>,
    T: FnMut(TreeInfo<TreeId, LeafId, Ctx>) -> TFut,
    TFut: IntoFuture<Item = (Ctx, TreeId), Error = Error>,
    TFut::Future: Send + 'static,
    L: FnMut(LeafInfo<LeafId, Leaf>) -> LFut,
    LFut: IntoFuture<Item = (Ctx, LeafId), Error = Error>,
    LFut::Future: Send + 'static,
    Ctx: Send + 'static,
{
    let event_id = EventId::new();
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
        {
            let ctx = ctx.clone();
            move |merge_node| merge(ctx.clone(), store.clone(), merge_node)
        },
        // fold, this function only creates entries from merge result and already merged subentries
        {
            let ctx = ctx.clone();
            move |merge_result, subentries| match merge_result {
                MergeResult::Reuse { name, entry } => {
                    future::ok(Some((name, None, entry))).boxify()
                }
                MergeResult::Delete => future::ok(None).boxify(),
                MergeResult::CreateTree {
                    name,
                    path,
                    parents,
                } => {
                    let subentries: BTreeMap<_, _> = subentries
                        .flatten()
                        .filter_map(
                            |(name, context, entry): (Option<MPathElement>, Option<Ctx>, _)| {
                                name.map(move |name| (name, (context, entry)))
                            },
                        )
                        .collect();
                    if subentries.is_empty() {
                        future::ok(None).boxify()
                    } else {
                        create_tree(TreeInfo {
                            path: path.clone(),
                            parents,
                            subentries,
                        })
                        .into_future()
                        .map(move |(context, tree_id)| {
                            Some((name, Some(context), Entry::Tree(tree_id)))
                        })
                        .traced_with_id(
                            &ctx.trace(),
                            "derive_manifest::create_tree",
                            trace_args! {
                                "path" => MPath::display_opt(path.as_ref()).to_string(),
                            },
                            event_id,
                        )
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
                    path: path.clone(),
                    parents,
                })
                .into_future()
                .map(move |(context, leaf_id)| Some((name, Some(context), Entry::Leaf(leaf_id))))
                .traced_with_id(
                    &ctx.trace(),
                    "derive_manifest::create_leaf",
                    trace_args! {
                        "path" => path.to_string(),
                    },
                    event_id,
                )
                .boxify(),
            }
        },
    )
    .map(|result: Option<_>| result.and_then(|(_, _, entry)| entry.into_tree()))
    .traced_with_id(&ctx.trace(), "derive_manifest", None, event_id)
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

fn merge<TreeId, LeafId, Leaf, Store>(
    ctx: CoreContext,
    store: Store,
    node: MergeNode<TreeId, LeafId, Leaf>,
) -> impl Future<
    Item = (
        MergeResult<TreeId, LeafId, Leaf>,
        Vec<MergeNode<TreeId, LeafId, Leaf>>,
    ),
    Error = Error,
>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Clone + Eq + Hash + fmt::Debug,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId>,
{
    let MergeNode {
        name,
        path,
        changes: PathTree {
            value: change,
            subentries,
        },
        mut parents,
    } = node;

    // Deduplicate entries in parents list, **preserving order** of entries.
    // Essentially performing a trivial merge between identical entries.
    {
        let mut visited = HashSet::new();
        parents.retain(|parent| visited.insert(parent.clone()));
    }

    // Apply change
    // If we create `parent_subtrees` (none of the return statement have been reached), this
    // indicates that file/tree conflict if any, has been resolved in favour of tree merge.
    let parent_subtrees = match change {
        None => match parents.as_slice() {
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
                            vec![tree_id.clone()]
                        }
                    }
                } else {
                    // We have single entry and do not have any changes associated with it subentries,
                    // it is safe to reuse current entry as is.
                    return future::ok((
                        MergeResult::Reuse {
                            name,
                            entry: parent_entry.clone(),
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
                        Entry::Leaf(leaf) => leaves.push(leaf.clone()),
                        Entry::Tree(tree) => trees.push(tree.clone()),
                    }
                }

                if leaves.is_empty() {
                    // We do not have any leaves at this point, and should proceed with
                    // merging of trees
                    trees
                } else if trees.is_empty() && subentries.is_empty() {
                    // We have leaves only but their ids are not equal to each other,
                    // this should immediately indicate conflict, as mercurial can successfully
                    // merge these leaves if they have identical content.
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
            .map(move |tree_id| tree_id.load(ctx.clone(), &store))
            .collect::<Vec<_>>(),
    )
    .from_err()
    .map(move |manifests| {
        let mut deps: BTreeMap<MPathElement, _> = Default::default();
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
