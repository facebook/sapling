/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::iter::Iterator;
use std::sync::Arc;

use anyhow::Result;
use anyhow::format_err;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::channel::mpsc;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Either;
use mononoke_macros::mononoke;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;
use mononoke_types::prefix_tree::PrefixTree;
use smallvec::SmallVec;
use smallvec::smallvec;

use crate::Entry;
use crate::Manifest;
use crate::ManifestParentReplacement;
use crate::PathTree;
use crate::StoreLoadable;
use crate::TrieMapOps;

/// Information passed to `create_tree` function when tree node is constructed
///
/// `Ctx` is any additional data which is useful for particular implementation of
/// manifest. It is `Some` for subentries for which `create_{tree|leaf}` was called to
/// generate them, and `None` if subentry was reused from one of its parents.
pub struct TreeInfo<TreeId, Leaf, Ctx, TrieMapType> {
    pub path: MPath,
    pub parents: Vec<TreeId>,
    pub subentries: TreeInfoSubentries<TreeId, Leaf, Ctx, TrieMapType>,
}

/// Represents the subentries of a tree node as a combination of singular subentries and
/// reused parent maps. The key for singular subentries is their name, while the key for
/// reused maps is a prefix that's prepended to all of the keys of the map.
pub type TreeInfoSubentries<TreeId, Leaf, Ctx, TrieMapType> =
    BTreeMap<SmallVec<[u8; 24]>, Either<(Option<Ctx>, Entry<TreeId, Leaf>), TrieMapType>>;

pub async fn flatten_subentries<Store, TreeId, Leaf, Ctx, TrieMapType>(
    ctx: &CoreContext,
    blobstore: &Store,
    subentries: TreeInfoSubentries<TreeId, Leaf, Ctx, TrieMapType>,
) -> Result<
    impl Iterator<Item = (MPathElement, (Option<Ctx>, Entry<TreeId, Leaf>))>
    + use<Store, TreeId, Leaf, Ctx, TrieMapType>,
>
where
    TrieMapType: TrieMapOps<Store, Entry<TreeId, Leaf>>,
{
    Ok(stream::iter(subentries)
        .map(|(prefix, entry_or_map)| async move {
            match entry_or_map {
                Either::Left((ctx, entry)) => {
                    Ok(vec![(MPathElement::from_smallvec(prefix)?, (ctx, entry))])
                }
                Either::Right(map) => map
                    .into_stream(ctx, blobstore)
                    .await?
                    .map_ok(|(mut path, entry)| {
                        path.insert_from_slice(0, prefix.as_ref());
                        Ok((MPathElement::from_smallvec(path)?, (None, entry)))
                    })
                    .try_collect::<Vec<_>>()
                    .await?
                    .into_iter()
                    .collect::<Result<_>>(),
            }
        })
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten())
}

/// Information passed to `create_leaf` function when leaf node is constructed
pub struct LeafInfo<Leaf, LeafChange> {
    pub path: NonRootMPath,
    pub parents: Vec<Leaf>,
    /// Leaf value, if it is not provided it means we have leaves only conflict
    /// which can potentially be resolved by `create_leaf`, in case of mercurial
    /// multiple leaves with the same content can be successfully resolved.
    pub change: Option<LeafChange>,
}

/// Derive a new manifest from parents and a set of changes. The types have to match in the
/// following way here:
///
/// - We'll walk and merge the manifests from the parents. Those must be Manifests where trees are
///   `TreeId` and leaves are `Leaf`.
/// - We'll create new leaves (for the diff) through create_leaf (which should merge changes), and
///   new trees through create_tree, which will receive entries consisting of existing trees and
///   trees merged with new leaves and trees (and should produce a new tree).
/// - To make this work, `create_tree` must return the same kind of `TreeId` as the ones that exist in
///   the tree currently, and `create_leaf` must return the same kind of `Leaf`.
pub fn derive_manifest<LeafChange, TreeId, Leaf, T, TFut, L, LFut, Ctx, Store>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<LeafChange>)>,
    subtree_changes: impl IntoIterator<Item = ManifestParentReplacement<TreeId, Leaf>>,
    create_tree: T,
    create_leaf: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    Store: Sync + Send + Clone + 'static,
    LeafChange: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    T: Fn(TreeInfo<TreeId, Leaf, Ctx, <TreeId::Value as Manifest<Store>>::TrieMapType>) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<Leaf, LeafChange>) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, Leaf)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, Leaf>> + Send + Sync + 'static,
    Ctx: Send + 'static,
{
    derive_manifest_inner(
        ctx,
        store,
        parents,
        changes,
        subtree_changes,
        create_tree,
        create_leaf,
    )
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
pub fn derive_manifest_inner<LeafChange, TreeId, Leaf, T, TFut, L, LFut, Ctx, Store>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<LeafChange>)>,
    subtree_changes: impl IntoIterator<Item = ManifestParentReplacement<TreeId, Leaf>>,
    create_tree: T,
    create_leaf: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    Store: Sync + Send + Clone + 'static,
    LeafChange: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    T: Fn(TreeInfo<TreeId, Leaf, Ctx, <TreeId::Value as Manifest<Store>>::TrieMapType>) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<Leaf, LeafChange>) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, Leaf)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, Leaf>> + Send + 'static,
    Ctx: Send + 'static,
{
    bounded_traversal::bounded_traversal(
        256,
        MergeNode {
            name: None,
            path: MPath::ROOT,
            changes: PathTree::from_iter(
                changes
                    .into_iter()
                    .map(|(path, change)| (path, Some(Change::from(change)))),
            ),
            parents: parents.into_iter().map(Entry::Tree).collect(),
            parent_replacements: PathTree::from_iter(
                subtree_changes
                    .into_iter()
                    .map(|r| (r.path, Some(r.replacements))),
            ),
        },
        // unfold, all merge logic happens in this unfold function
        move |merge_node: MergeNode<_, Leaf, LeafChange>| {
            merge(ctx.clone(), store.clone(), merge_node).boxed()
        },
        // fold, this function only creates entries from merge result and already merged subentries
        {
            let create_tree = Arc::new(create_tree);
            let create_leaf = Arc::new(create_leaf);
            move |merge_result: MergeResult<_, Leaf, LeafChange, _>, subentries| {
                let create_tree = create_tree.clone();
                let create_leaf = create_leaf.clone();
                async move {
                    mononoke::spawn_task(async move {
                        match merge_result {
                            MergeResult::Reuse { name, entry } => Ok(Some((name, None, entry))),
                            MergeResult::Delete => Ok(None),
                            MergeResult::CreateTree {
                                name,
                                path,
                                parents,
                                reused_maps,
                            } => {
                                let mut subentries = subentries
                                    .flatten()
                                    .filter_map(
                                        |(name, context, entry): (
                                            Option<MPathElement>,
                                            Option<Ctx>,
                                            Entry<TreeId, Leaf>,
                                        )| {
                                            name.map(move |name| (name, (context, entry)))
                                        },
                                    )
                                    .peekable();

                                if subentries.peek().is_none() && reused_maps.is_empty() {
                                    Ok(None)
                                } else {
                                    let subentries = subentries
                                        .map(|(name, (context, entry))| {
                                            (name.to_smallvec(), Either::Left((context, entry)))
                                        })
                                        .chain(
                                            reused_maps
                                                .into_iter()
                                                .map(|(prefix, map)| (prefix, Either::Right(map))),
                                        )
                                        .collect();

                                    let (context, tree_id) = create_tree(TreeInfo {
                                        path: path.clone(),
                                        parents,
                                        subentries,
                                    })
                                    .await?;
                                    Ok(Some((name, Some(context), Entry::Tree(tree_id))))
                                }
                            }
                            MergeResult::CreateLeaf {
                                change,
                                name,
                                path,
                                parents,
                            } => {
                                let (context, leaf_id) = create_leaf(LeafInfo {
                                    change,
                                    path: path.clone(),
                                    parents,
                                })
                                .await?;
                                Ok(Some((name, Some(context), Entry::Leaf(leaf_id))))
                            }
                        }
                    })
                    .await?
                }
                .boxed()
            }
        },
    )
    .map_ok(|result: Option<_>| result.and_then(|(_, _, entry)| entry.into_tree()))
}

type BoxFuture<T> = future::BoxFuture<'static, Result<T>>;

/// A convenience wrapper around `derive_manifest` that allows for the tree and leaf creation
/// closures to send IO work onto a channel that is then fed into a buffered stream. NOTE: don't
/// send computationally expensive work as it will block the task.
///
/// The sender is commonly used to write blobs to the blobstore concurrently.
///
/// `derive_manifest_with_work_sender` guarantees that all work is completed before it returns, but
/// it does not guarantee the order in which the work is completed.
pub fn derive_manifest_with_io_sender<LeafChange, TreeId, Leaf, T, TFut, L, LFut, Ctx, Store>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<LeafChange>)>,
    subtree_changes: impl IntoIterator<Item = ManifestParentReplacement<TreeId, Leaf>>,
    create_tree_with_sender: T,
    create_leaf_with_sender: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    LeafChange: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    T: Fn(
            TreeInfo<TreeId, Leaf, Ctx, <TreeId::Value as Manifest<Store>>::TrieMapType>,
            mpsc::UnboundedSender<BoxFuture<()>>,
        ) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<Leaf, LeafChange>, mpsc::UnboundedSender<BoxFuture<()>>) -> LFut
        + Send
        + Sync
        + 'static,
    LFut: Future<Output = Result<(Ctx, Leaf)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, Leaf>> + Send + 'static,
    Ctx: Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded();

    let derive = derive_manifest_inner(
        ctx,
        store,
        parents,
        changes,
        subtree_changes,
        {
            cloned!(sender);
            move |tree_info| create_tree_with_sender(tree_info, sender.clone())
        },
        {
            cloned!(sender);
            move |leaf_info| create_leaf_with_sender(leaf_info, sender.clone())
        },
    );
    let process = receiver
        .buffer_unordered(1024)
        .try_for_each(|_| future::ok(()));

    future::try_join(derive, process).map_ok(|(res, ())| res)
}

// Change is isomorphic to Option, but it makes it easier to understand merge logic
enum Change<LeafChange> {
    Add(LeafChange),
    Remove,
}

impl<Leaf> From<Option<Leaf>> for Change<Leaf> {
    fn from(change: Option<Leaf>) -> Self {
        change.map_or(Change::Remove, Change::Add)
    }
}

enum MergeResult<TreeId, Leaf, LeafChange, TrieMapType> {
    Delete,
    Reuse {
        name: Option<MPathElement>,
        entry: Entry<TreeId, Leaf>,
    },
    CreateLeaf {
        change: Option<LeafChange>,
        name: Option<MPathElement>,
        path: NonRootMPath,
        parents: Vec<Leaf>,
    },
    CreateTree {
        name: Option<MPathElement>,
        path: MPath,
        parents: Vec<TreeId>,
        reused_maps: Vec<(SmallVec<[u8; 24]>, TrieMapType)>,
    },
}

/// This node represents unmerged state of `parents.len() + 1` way merge
/// between changes and parents.
struct MergeNode<TreeId, Leaf, LeafChange> {
    name: Option<MPathElement>, // name of this node in parent manifest
    path: MPath,                // path to this node from root of the manifest
    changes: PathTree<Option<Change<LeafChange>>>, // changes associated with current subtree
    parents: Vec<Entry<TreeId, Leaf>>, // unmerged parents of current node
    parent_replacements: PathTree<Option<Vec<Entry<TreeId, Leaf>>>>,
}

async fn merge<TreeId, Leaf, LeafChange, Store>(
    ctx: CoreContext,
    store: Store,
    node: MergeNode<TreeId, Leaf, LeafChange>,
) -> Result<(
    MergeResult<TreeId, Leaf, LeafChange, <TreeId::Value as Manifest<Store>>::TrieMapType>,
    Vec<MergeNode<TreeId, Leaf, LeafChange>>,
)>
where
    Store: Sync + Send + Clone + 'static,
    LeafChange: Send + Clone + Eq + Hash + fmt::Debug,
    Leaf: Send + Clone + Eq + Hash + fmt::Debug,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync,
    TreeId::Value: Manifest<Store, TreeId = TreeId, Leaf = Leaf>,
    <TreeId::Value as Manifest<Store>>::TrieMapType: TrieMapOps<Store, Entry<TreeId, Leaf>>,
{
    let MergeNode {
        name,
        path,
        changes: PathTree {
            value: change,
            subentries,
        },
        mut parents,
        parent_replacements:
            PathTree {
                value: parent_replacements_value,
                subentries: parent_replacements_subentries,
            },
    } = node;

    if let Some(new_parents) = parent_replacements_value {
        parents = new_parents;
    }

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
                if !subentries.is_empty() || !parent_replacements_subentries.is_empty() {
                    match parent_entry {
                        Entry::Leaf(_) => {
                            // Current entry is a leaf but we still have changes that needs
                            // to be applied to its subentries, we cannot resolve this merge.
                            let error = format_err!(
                                "Can not apply changes to a leaf:\npath: {:?}\nparents: {:?}",
                                path,
                                parents
                            );
                            return Err(error);
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
                    return Ok((
                        MergeResult::Reuse {
                            name,
                            entry: parent_entry.clone(),
                        },
                        Vec::new(),
                    ));
                }
            }
            _ => {
                // Split entries into leaves and trees.
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
                } else if trees.is_empty()
                    && subentries.is_empty()
                    && parent_replacements_subentries.is_empty()
                {
                    // We have leaves only but their ids are not equal to each other,
                    // this should immediately indicate conflict, as mercurial can successfully
                    // merge these leaves if they have identical content.
                    return Ok((
                        MergeResult::CreateLeaf {
                            change: None,
                            name,
                            path: path
                                .into_optional_non_root_path()
                                .expect("leaf can not have empty path"),
                            parents: leaves,
                        },
                        Vec::new(),
                    ));
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
                    return Err(error);
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
            return Ok((
                MergeResult::CreateLeaf {
                    change: Some(leaf),
                    name,
                    path: path
                        .into_optional_non_root_path()
                        .expect("leaf can not have empty path"),
                    parents: parents.into_iter().filter_map(Entry::into_leaf).collect(),
                },
                Vec::new(),
            ));
        }
    };

    if parent_subtrees.is_empty()
        && subentries.is_empty()
        && parent_replacements_subentries.is_empty()
    {
        // All elements of this merge tree have been deleted.
        // Nothing left to do apart from indicating that this node needs to be removed
        // from its parent.
        return Ok((MergeResult::Delete, Vec::new()));
    }

    // Fetch parent trees and merge them.
    borrowed!(ctx, store);
    let parent_manifests_trie_maps =
        future::try_join_all(parent_subtrees.iter().map(move |tree_id| {
            cloned!(ctx);
            async move {
                tree_id
                    .load(&ctx, store)
                    .await?
                    .into_trie_map(&ctx, store)
                    .await
            }
        }))
        .await?;

    let MergeSubentriesResult {
        reused_maps,
        merge_nodes,
    } = merge_subentries(
        ctx,
        store,
        &path,
        subentries,
        parent_manifests_trie_maps,
        parent_replacements_subentries,
    )
    .await?;

    Ok((
        MergeResult::CreateTree {
            name,
            path,
            parents: parent_subtrees,
            reused_maps,
        },
        merge_nodes,
    ))
}

struct MergeSubentriesNode<'a, TreeId, Leaf, LeafChange, TrieMapType> {
    path: &'a MPath,
    prefix: SmallVec<[u8; 24]>,
    changes: PrefixTree<PathTree<Option<Change<LeafChange>>>>,
    parents: Vec<TrieMapType>,
    parent_replacements: PrefixTree<PathTree<Option<Vec<Entry<TreeId, Leaf>>>>>,
}

struct MergeSubentriesResult<TreeId, Leaf, LeafChange, TrieMapType> {
    reused_maps: Vec<(SmallVec<[u8; 24]>, TrieMapType)>,
    merge_nodes: Vec<MergeNode<TreeId, Leaf, LeafChange>>,
}

async fn merge_subentries<TreeId, Leaf, LeafChange, TrieMapType, Store>(
    ctx: &CoreContext,
    store: &Store,
    path: &MPath,
    changes: PrefixTree<PathTree<Option<Change<LeafChange>>>>,
    parents: Vec<TrieMapType>,
    parent_replacements: PrefixTree<PathTree<Option<Vec<Entry<TreeId, Leaf>>>>>,
) -> Result<MergeSubentriesResult<TreeId, Leaf, LeafChange, TrieMapType>>
where
    TrieMapType: TrieMapOps<Store, Entry<TreeId, Leaf>> + Send,
    Store: Sync + Send,
    TreeId: Send + Clone,
    Leaf: Send + Clone,
    LeafChange: Send + Clone,
{
    bounded_traversal::bounded_traversal(
        256,
        MergeSubentriesNode {
            path,
            prefix: smallvec![],
            changes,
            parents,
            parent_replacements,
        },
        move |MergeSubentriesNode::<_, _, _, _> {
                  path,
                  prefix,
                  changes,
                  parents,
                  parent_replacements,
              }| {
            async move {
                // If there are no changes and only one parent then we can reuse the parent's map.
                // TODO(youssefsalama): In case of multiple identical parent maps, reuse one of their maps. This
                // will only become possible once sharded map nodes are extended with aggregate information.
                if changes.is_empty() && parents.len() <= 1 && parent_replacements.is_empty() {
                    return Ok((
                        MergeSubentriesResult {
                            reused_maps: parents
                                .into_iter()
                                .next()
                                .map(|parent| (prefix, parent))
                                .into_iter()
                                .collect(),
                            merge_nodes: vec![],
                        },
                        vec![],
                    ));
                }

                // Expand changes and parent maps by the first byte, group changes and parent subentries that
                // correspond to the current prefix into current_merge_node, then recurse on changes and parent
                // maps that start with each byte, accumulating the resulting merge nodes and reused maps.

                let mut child_merge_subentries_nodes: BTreeMap<
                    u8,
                    MergeSubentriesNode<_, _, _, _>,
                > = Default::default();
                let mut current_merge_node = None;

                let (current_change, child_changes) = changes.expand();

                if let Some(current_change) = current_change {
                    let name = MPathElement::new_from_slice(&prefix)?;
                    current_merge_node = Some(MergeNode {
                        path: path.join_element(Some(&name)),
                        name: Some(name),
                        changes: current_change,
                        parents: Default::default(),
                        parent_replacements: Default::default(),
                    })
                }

                for (next_byte, changes) in child_changes {
                    child_merge_subentries_nodes
                        .entry(next_byte)
                        .or_insert_with(|| MergeSubentriesNode {
                            path,
                            prefix: prefix
                                .iter()
                                .copied()
                                .chain(std::iter::once(next_byte))
                                .collect(),
                            changes: Default::default(),
                            parents: Default::default(),
                            parent_replacements: Default::default(),
                        })
                        .changes = changes;
                }

                for parent in parents {
                    let (current_entry, child_trie_maps) = parent.expand(ctx, store).await?;

                    if let Some(current_entry) = current_entry {
                        let name = MPathElement::new_from_slice(&prefix)?;

                        current_merge_node
                            .get_or_insert_with(|| MergeNode {
                                path: path.join(Some(&name)),
                                name: Some(name),
                                changes: Default::default(),
                                parents: Default::default(),
                                parent_replacements: Default::default(),
                            })
                            .parents
                            .push(current_entry.clone());
                    }

                    for (next_byte, trie_map) in child_trie_maps {
                        child_merge_subentries_nodes
                            .entry(next_byte)
                            .or_insert_with(|| MergeSubentriesNode {
                                path,
                                prefix: prefix
                                    .iter()
                                    .copied()
                                    .chain(std::iter::once(next_byte))
                                    .collect(),
                                changes: Default::default(),
                                parents: Default::default(),
                                parent_replacements: Default::default(),
                            })
                            .parents
                            .push(trie_map);
                    }
                }

                let (current_parent_replacements, child_parent_replacements) =
                    parent_replacements.expand();

                if let Some(current_parent_replacements) = current_parent_replacements {
                    let name = MPathElement::new_from_slice(&prefix)?;

                    current_merge_node
                        .get_or_insert_with(|| MergeNode {
                            path: path.join_element(Some(&name)),
                            name: Some(name),
                            changes: Default::default(),
                            parents: Default::default(),
                            parent_replacements: Default::default(),
                        })
                        .parent_replacements = current_parent_replacements;
                }

                for (next_byte, parent_replacements) in child_parent_replacements {
                    child_merge_subentries_nodes
                        .entry(next_byte)
                        .or_insert_with(|| MergeSubentriesNode {
                            path,
                            prefix: prefix
                                .iter()
                                .copied()
                                .chain(std::iter::once(next_byte))
                                .collect(),
                            changes: Default::default(),
                            parents: Default::default(),
                            parent_replacements: Default::default(),
                        })
                        .parent_replacements = parent_replacements;
                }

                Ok((
                    MergeSubentriesResult {
                        reused_maps: vec![],
                        merge_nodes: current_merge_node.into_iter().collect::<Vec<_>>(),
                    },
                    child_merge_subentries_nodes.into_values().collect(),
                ))
            }
            .boxed()
        },
        |mut result,
         child_results: std::iter::Flatten<
            std::vec::IntoIter<Option<MergeSubentriesResult<_, _, _, _>>>,
        >| {
            async {
                for child_result in child_results {
                    result.reused_maps.extend(child_result.reused_maps);
                    result.merge_nodes.extend(child_result.merge_nodes);
                }
                Ok(result)
            }
            .boxed()
        },
    )
    .await
}
