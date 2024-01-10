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

use anyhow::format_err;
use anyhow::Result;
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
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;
use smallvec::smallvec;
use smallvec::SmallVec;

use crate::AsyncManifest as Manifest;
use crate::Entry;
use crate::PathTree;
use crate::StoreLoadable;
use crate::TrieMapOps;

/// Information passed to `create_tree` function when tree node is constructed
///
/// `Ctx` is any additional data which is useful for particular implementation of
/// manifest. It is `Some` for subentries for which `create_{tree|leaf}` was called to
/// generate them, and `None` if subentry was reused from one of its parents.
pub struct TreeInfo<TreeId, LeafId, Ctx, TrieMapType> {
    pub path: Option<NonRootMPath>,
    pub parents: Vec<TreeId>,
    pub subentries: TreeInfoSubentries<TreeId, LeafId, Ctx, TrieMapType>,
}

pub enum TreeInfoSubentries<TreeId, LeafId, Ctx, TrieMapType> {
    AllSubentries(BTreeMap<MPathElement, (Option<Ctx>, Entry<TreeId, LeafId>)>),
    ReusedMapsAndSubentries {
        produced_subentries_and_reused_maps:
            BTreeMap<SmallVec<[u8; 24]>, Either<(Option<Ctx>, Entry<TreeId, LeafId>), TrieMapType>>,
        /// Consists of all subentries of parents that are not contained in any reused map.
        consumed_subentries: Vec<Entry<TreeId, LeafId>>,
    },
}

impl<TreeId, LeafId, Ctx, TrieMapType> Default
    for TreeInfoSubentries<TreeId, LeafId, Ctx, TrieMapType>
{
    fn default() -> Self {
        TreeInfoSubentries::AllSubentries(Default::default())
    }
}

pub async fn flatten_subentries<Store, TreeId, IntermediateLeafId, LeafId, Ctx, TrieMapType>(
    ctx: &CoreContext,
    blobstore: &Store,
    subentries: TreeInfoSubentries<TreeId, IntermediateLeafId, Ctx, TrieMapType>,
) -> Result<
    impl Iterator<
        Item = (
            MPathElement,
            (Option<Ctx>, Entry<TreeId, IntermediateLeafId>),
        ),
    >,
>
where
    TrieMapType: TrieMapOps<Store, Entry<TreeId, LeafId>>,
    IntermediateLeafId: From<LeafId>,
{
    match subentries {
        TreeInfoSubentries::AllSubentries(subentries) => Ok(Either::Left(subentries.into_iter())),
        TreeInfoSubentries::ReusedMapsAndSubentries {
            produced_subentries_and_reused_maps,
            consumed_subentries: _,
        } => {
            let subentries_futures = produced_subentries_and_reused_maps
                .into_iter()
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
                                Ok((
                                    MPathElement::from_smallvec(path)?,
                                    (None, convert_to_intermediate_entry(entry)),
                                ))
                            })
                            .try_collect::<Vec<_>>()
                            .await?
                            .into_iter()
                            .collect::<Result<_>>(),
                    }
                })
                .collect::<Vec<_>>();

            Ok(Either::Right(
                stream::iter(subentries_futures)
                    .buffer_unordered(100)
                    .try_collect::<Vec<_>>()
                    .await?
                    .into_iter()
                    .flatten(),
            ))
        }
    }
}

/// Information passed to `create_leaf` function when leaf node is constructed
pub struct LeafInfo<LeafId, Leaf> {
    pub path: NonRootMPath,
    pub parents: Vec<LeafId>,
    /// Leaf value, if it is not provided it means we have leaves only conflict
    /// which can potentially be resolved by `create_leaf`, in case of mercurial
    /// multiple leaves with the same content can be successfully resolved.
    pub leaf: Option<Leaf>,
}

/// Derive a new manifest from parents and a set of changes. The types have to match in the
/// following way here:
///
/// - We'll walk and merge the manifests from the parents. Those must be Manifests where trees are
/// TreeId and leaves are LeafId.
/// - We'll create new leaves (for the diff) through create_leaf (which should merge changes), and
/// new tres through create_tree, which will receive entries consisting of existing trees and tres
/// merged with new leaves and trees (and should produce a new tree).
/// - To make this work, create_tree must return the same kind of TreeId as the ones that exist in
/// the tree currently. That said, this constraint is marginally relaxed for leaves: create_leaf
/// can return an IntermediateLeafId that must, and that is the also the type that create_tree will
/// receive for leaves (to make this work, IntermediateLeafId must implement From<LeafId> so that
/// leaves that are to be reused from the existing tree can be turned into IntermediateLeafId).
///
/// Note that for most use cases, IntermediateLeafId and LeafId should probably be the same type.
/// That said, this distinction can be useful in cases where the leaves aren't actually objects
/// that exist in the blobstore, and are just contained in trees. This is notably the case with
/// Fsnodes, where leaves are FsnodeFiles and are actually stored in their parent manifest.
pub fn derive_manifest<TreeId, LeafId, IntermediateLeafId, Leaf, T, TFut, L, LFut, Ctx, Store>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<Leaf>)>,
    create_tree: T,
    create_leaf: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + 'static,
    IntermediateLeafId: Send + From<LeafId> + 'static + fmt::Debug + Clone + Eq + Hash,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, LeafId = LeafId>,
    <TreeId as StoreLoadable<Store>>::Value: Send + Sync,
    T: Fn(
            TreeInfo<
                TreeId,
                IntermediateLeafId,
                Ctx,
                <TreeId::Value as Manifest<Store>>::TrieMapType,
            >,
        ) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<IntermediateLeafId, Leaf>) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, IntermediateLeafId)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, LeafId>> + Send + Sync + 'static,
    Ctx: Send + 'static,
{
    derive_manifest_inner(ctx, store, parents, changes, create_tree, create_leaf)
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
pub fn derive_manifest_inner<
    TreeId,
    LeafId,
    IntermediateLeafId,
    Leaf,
    T,
    TFut,
    L,
    LFut,
    Ctx,
    Store,
>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<Leaf>)>,
    create_tree: T,
    create_leaf: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + 'static,
    IntermediateLeafId: Send + From<LeafId> + 'static + Eq + Hash + Clone + fmt::Debug,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, LeafId = LeafId>,
    <TreeId as StoreLoadable<Store>>::Value: Send + Sync,
    T: Fn(
            TreeInfo<
                TreeId,
                IntermediateLeafId,
                Ctx,
                <TreeId::Value as Manifest<Store>>::TrieMapType,
            >,
        ) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<IntermediateLeafId, Leaf>) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, IntermediateLeafId)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, LeafId>> + Send + 'static,
    Ctx: Send + 'static,
{
    bounded_traversal::bounded_traversal(
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
        move |merge_node: MergeNode<_, IntermediateLeafId, _>| {
            merge(ctx.clone(), store.clone(), merge_node).boxed()
        },
        // fold, this function only creates entries from merge result and already merged subentries
        {
            let create_tree = Arc::new(create_tree);
            let create_leaf = Arc::new(create_leaf);
            move |merge_result: MergeResult<_, IntermediateLeafId, _, _>, subentries| {
                let create_tree = create_tree.clone();
                let create_leaf = create_leaf.clone();
                async move {
                    tokio::spawn(async move {
                        match merge_result {
                            MergeResult::Reuse { name, entry } => {
                                Ok(Some((name, None, convert_to_intermediate_entry(entry))))
                            }
                            MergeResult::Delete => Ok(None),
                            MergeResult::CreateTree {
                                name,
                                path,
                                parents,
                                reused_maps,
                                consumed_subentries,
                            } => {
                                let mut subentries = subentries
                                    .flatten()
                                    .filter_map(
                                        |(name, context, entry): (
                                            Option<MPathElement>,
                                            Option<Ctx>,
                                            Entry<TreeId, IntermediateLeafId>,
                                        )| {
                                            name.map(move |name| (name, (context, entry)))
                                        },
                                    )
                                    .peekable();

                                if subentries.peek().is_none() && reused_maps.is_empty() {
                                    Ok(None)
                                } else {
                                    let subentries = match reused_maps.is_empty() {
                                        true => {
                                            TreeInfoSubentries::AllSubentries(subentries.collect())
                                        }
                                        false => TreeInfoSubentries::ReusedMapsAndSubentries {
                                            produced_subentries_and_reused_maps: subentries
                                                .map(|(name, (context, entry))| {
                                                    (
                                                        name.to_smallvec(),
                                                        Either::Left((context, entry)),
                                                    )
                                                })
                                                .chain(reused_maps.into_iter().map(
                                                    |(prefix, map)| (prefix, Either::Right(map)),
                                                ))
                                                .collect(),
                                            consumed_subentries,
                                        },
                                    };

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
                                leaf,
                                name,
                                path,
                                parents,
                            } => {
                                let (context, leaf_id) = create_leaf(LeafInfo {
                                    leaf,
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

fn convert_to_intermediate_entry<TreeId, LeafId, IntermediateLeafId>(
    e: Entry<TreeId, LeafId>,
) -> Entry<TreeId, IntermediateLeafId>
where
    IntermediateLeafId: From<LeafId>,
{
    match e {
        Entry::Tree(t) => Entry::Tree(t),
        Entry::Leaf(l) => Entry::Leaf(l.into()),
    }
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
pub fn derive_manifest_with_io_sender<
    TreeId,
    LeafId,
    IntermediateLeafId,
    Leaf,
    T,
    TFut,
    L,
    LFut,
    Ctx,
    Store,
>(
    ctx: CoreContext,
    store: Store,
    parents: impl IntoIterator<Item = TreeId>,
    changes: impl IntoIterator<Item = (NonRootMPath, Option<Leaf>)>,
    create_tree_with_sender: T,
    create_leaf_with_sender: L,
) -> impl Future<Output = Result<Option<TreeId>>>
where
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    Leaf: Send + 'static,
    IntermediateLeafId: Send + From<LeafId> + 'static + Eq + Hash + fmt::Debug + Clone,
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<Store, TreeId = TreeId, LeafId = LeafId>,
    <TreeId as StoreLoadable<Store>>::Value: Send + Sync,
    T: Fn(
            TreeInfo<
                TreeId,
                IntermediateLeafId,
                Ctx,
                <TreeId::Value as Manifest<Store>>::TrieMapType,
            >,
            mpsc::UnboundedSender<BoxFuture<()>>,
        ) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId)>> + Send + 'static,
    L: Fn(LeafInfo<IntermediateLeafId, Leaf>, mpsc::UnboundedSender<BoxFuture<()>>) -> LFut
        + Send
        + Sync
        + 'static,
    LFut: Future<Output = Result<(Ctx, IntermediateLeafId)>> + Send + 'static,
    <TreeId::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, LeafId>> + Send + 'static,
    Ctx: Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded();

    let derive = derive_manifest_inner(
        ctx,
        store,
        parents,
        changes,
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
enum Change<LeafId> {
    Add(LeafId),
    Remove,
}

impl<Leaf> From<Option<Leaf>> for Change<Leaf> {
    fn from(change: Option<Leaf>) -> Self {
        change.map_or(Change::Remove, Change::Add)
    }
}

enum MergeResult<TreeId, LeafId, Leaf, TrieMapType> {
    Delete,
    Reuse {
        name: Option<MPathElement>,
        entry: Entry<TreeId, LeafId>,
    },
    CreateLeaf {
        leaf: Option<Leaf>,
        name: Option<MPathElement>,
        path: NonRootMPath,
        parents: Vec<LeafId>,
    },
    CreateTree {
        name: Option<MPathElement>,
        path: Option<NonRootMPath>,
        parents: Vec<TreeId>,
        reused_maps: Vec<(SmallVec<[u8; 24]>, TrieMapType)>,
        consumed_subentries: Vec<Entry<TreeId, LeafId>>,
    },
}

/// This node represents unmerged state of `parents.len() + 1` way merge
/// between changes and parents.
struct MergeNode<TreeId, LeafId, Leaf> {
    name: Option<MPathElement>, // name of this node in parent manifest
    path: Option<NonRootMPath>, // path to this node from root of the manifest
    changes: PathTree<Option<Change<Leaf>>>, // changes associated with current subtree
    parents: Vec<Entry<TreeId, LeafId>>, // unmerged parents of current node
}

async fn merge<TreeId, LeafId, IntermediateLeafId, Leaf, Store>(
    ctx: CoreContext,
    store: Store,
    node: MergeNode<TreeId, IntermediateLeafId, Leaf>,
) -> Result<(
    MergeResult<TreeId, IntermediateLeafId, Leaf, <TreeId::Value as Manifest<Store>>::TrieMapType>,
    Vec<MergeNode<TreeId, IntermediateLeafId, Leaf>>,
)>
where
    Store: Sync + Send + Clone + 'static,
    IntermediateLeafId: Send + From<LeafId> + 'static + fmt::Debug + Clone + Eq + Hash,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug,
    Leaf: Send,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync,
    TreeId::Value: Manifest<Store, TreeId = TreeId, LeafId = LeafId>,
    <TreeId::Value as Manifest<Store>>::TrieMapType: TrieMapOps<Store, Entry<TreeId, LeafId>>,
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
                    return Ok((
                        MergeResult::CreateLeaf {
                            leaf: None,
                            name,
                            path: path.expect("leaf can not have empty path"),
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
                    leaf: Some(leaf),
                    name,
                    path: path.expect("leaf can not have empty path"),
                    parents: parents.into_iter().filter_map(Entry::into_leaf).collect(),
                },
                Vec::new(),
            ));
        }
    };

    if parent_subtrees.is_empty() && subentries.is_empty() {
        // All elements of this merge tree have been deleted.
        // Nothing left to do apart from inidicating that this node needs to be removed
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
        consumed_subentries,
    } = merge_subentries(
        ctx,
        store,
        path.as_ref(),
        subentries,
        parent_manifests_trie_maps,
    )
    .await?;

    Ok((
        MergeResult::CreateTree {
            name,
            path,
            parents: parent_subtrees,
            reused_maps,
            consumed_subentries,
        },
        merge_nodes,
    ))
}

struct MergeSubentriesNode<'a, Leaf, TrieMapType> {
    path: Option<&'a NonRootMPath>,
    prefix: SmallVec<[u8; 24]>,
    changes: TrieMap<PathTree<Option<Change<Leaf>>>>,
    parents: Vec<TrieMapType>,
}

struct MergeSubentriesResult<TreeId, IntermediateLeafId, Leaf, TrieMapType> {
    reused_maps: Vec<(SmallVec<[u8; 24]>, TrieMapType)>,
    merge_nodes: Vec<MergeNode<TreeId, IntermediateLeafId, Leaf>>,
    consumed_subentries: Vec<Entry<TreeId, IntermediateLeafId>>,
}

async fn merge_subentries<TreeId, LeafId, IntermediateLeafId, Leaf, TrieMapType, Store>(
    ctx: &CoreContext,
    store: &Store,
    path: Option<&NonRootMPath>,
    changes: TrieMap<PathTree<Option<Change<Leaf>>>>,
    parents: Vec<TrieMapType>,
) -> Result<MergeSubentriesResult<TreeId, IntermediateLeafId, Leaf, TrieMapType>>
where
    TrieMapType: TrieMapOps<Store, Entry<TreeId, LeafId>> + Send,
    Store: Sync + Send,
    IntermediateLeafId: Send + From<LeafId> + Clone,
    TreeId: Send + Clone,
    LeafId: Send,
    Leaf: Send,
{
    bounded_traversal::bounded_traversal(
        256,
        MergeSubentriesNode {
            path,
            prefix: smallvec![],
            changes,
            parents,
        },
        move |MergeSubentriesNode::<_, _> {
                  path,
                  prefix,
                  changes,
                  parents,
              }| {
            async move {
                // If there are no changes and only one parent then we can reuse the parent's map.
                // TODO(youssefsalama): In case of multiple identical parent maps, reuse one of their maps. This
                // will only become possible once sharded map nodes are extended with aggregate information.
                if changes.is_empty() && parents.len() <= 1 {
                    return Ok((
                        MergeSubentriesResult {
                            reused_maps: parents
                                .into_iter()
                                .next()
                                .map(|parent| (prefix, parent))
                                .into_iter()
                                .collect(),
                            merge_nodes: vec![],
                            consumed_subentries: vec![],
                        },
                        vec![],
                    ));
                }

                // Expand changes and parent maps by the first byte, group changes and parent subentries that
                // correspond to the current prefix into current_merge_node, then recurse on changes and parent
                // maps that start with each byte, accumulating the resulting merge nodes and reused maps.

                let mut child_merge_subentries_nodes: BTreeMap<u8, MergeSubentriesNode<_, _>> =
                    Default::default();
                let mut current_merge_node = None;

                let (current_change, child_changes) = changes.expand();

                if let Some(current_change) = current_change {
                    let name = MPathElement::new_from_slice(&prefix)?;
                    current_merge_node = Some(MergeNode {
                        path: Some(NonRootMPath::join_opt_element(path, &name)),
                        name: Some(name),
                        changes: current_change,
                        parents: Default::default(),
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
                        })
                        .changes = changes;
                }

                let mut consumed_subentries = vec![];

                for parent in parents {
                    let (current_entry, child_trie_maps) = parent.expand(ctx, store).await?;

                    if let Some(current_entry) = current_entry {
                        let name = MPathElement::new_from_slice(&prefix)?;
                        let current_entry = convert_to_intermediate_entry(current_entry);

                        current_merge_node
                            .get_or_insert_with(|| MergeNode {
                                path: Some(NonRootMPath::join_opt_element(path, &name)),
                                name: Some(name),
                                changes: Default::default(),
                                parents: Default::default(),
                            })
                            .parents
                            .push(current_entry.clone());
                        consumed_subentries.push(current_entry);
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
                            })
                            .parents
                            .push(trie_map);
                    }
                }

                Ok((
                    MergeSubentriesResult {
                        reused_maps: vec![],
                        merge_nodes: current_merge_node.into_iter().collect::<Vec<_>>(),
                        consumed_subentries,
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
                    result
                        .consumed_subentries
                        .extend(child_result.consumed_subentries);
                }
                Ok(result)
            }
            .boxed()
        },
    )
    .await
}
