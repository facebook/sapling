/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::iter::Iterator;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use cloned::cloned;
use context::CoreContext;
use futures::future::FutureExt;
use futures::stream::TryStreamExt;
use mononoke_types::MPathElement;
use sorted_vector_map::SortedVectorMap;

use crate::AsyncManifest as Manifest;
use crate::Entry;
use crate::StoreLoadable;

/// Information passed to the `create_tree` function when a tree node is constructed.
///
/// `Ctx` is any additional data which is useful for particular implementation
/// of manifest.
pub struct FromPredecessorTreeInfo<NewTreeId, NewLeafId, OldTreeId, Ctx> {
    pub predecessor: OldTreeId,
    pub subentries: SortedVectorMap<MPathElement, (Ctx, Entry<NewTreeId, NewLeafId>)>,
}

/// Information passed to the `create_leaf` function when a leaf node is constructed.
pub struct FromPredecessorLeafInfo<OldLeafId> {
    pub predecessor: OldLeafId,
}

/// Derive a new manifest type from an old manifest type (e.g. creating a sharded manifest from
/// a non-sharded manifest that has the same content). Each tree in the old manifest is unfolded
/// into its subentries recursively, create_leaf is then called on each leaf to create the leaves
/// of the new manifest, and create_tree is called to create its trees.
pub async fn derive_manifest_from_predecessor<
    NewTreeId,
    NewLeafId,
    OldTreeId,
    OldLeafId,
    T,
    TFut,
    L,
    LFut,
    Ctx,
    Store,
>(
    ctx: CoreContext,
    store: Store,
    predecessor: OldTreeId,
    create_tree: T,
    create_leaf: L,
) -> Result<NewTreeId>
where
    Store: Sync + Send + Clone + 'static,
    OldLeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    NewLeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    OldTreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    OldTreeId::Value: Manifest<Store, TreeId = OldTreeId, LeafId = OldLeafId>,
    <OldTreeId as StoreLoadable<Store>>::Value: Send + Sync,
    NewTreeId: Send + Clone + Eq + Hash + fmt::Debug + 'static,
    T: Fn(FromPredecessorTreeInfo<NewTreeId, NewLeafId, OldTreeId, Ctx>) -> TFut
        + Send
        + Sync
        + 'static,
    TFut: Future<Output = Result<(Ctx, NewTreeId)>> + Send + 'static,
    L: Fn(FromPredecessorLeafInfo<OldLeafId>) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, NewLeafId)>> + Send + 'static,
    Ctx: Send + 'static,
{
    enum UnfoldNode<OldTreeId, OldLeafId> {
        Tree {
            predecessor: OldTreeId,
            parent_path_element: Option<MPathElement>,
        },
        Leaf {
            predecessor: OldLeafId,
            parent_path_element: Option<MPathElement>,
        },
    }

    enum FoldNode<OldTreeId, OldLeafId> {
        CreateTree {
            predecessor: OldTreeId,
            parent_path_element: Option<MPathElement>,
        },
        CreateLeaf {
            predecessor: OldLeafId,
            parent_path_element: Option<MPathElement>,
        },
    }

    let (_, (_, entry)) = bounded_traversal::bounded_traversal(
        256,
        UnfoldNode::Tree {
            predecessor,
            parent_path_element: None,
        },
        move |unfold_node: UnfoldNode<OldTreeId, OldLeafId>| {
            cloned!(ctx, store);
            async move {
                cloned!(ctx, store);
                match unfold_node {
                    UnfoldNode::Tree {
                        predecessor,
                        parent_path_element,
                    } => {
                        let old_tree = predecessor.load(&ctx, &store).await?;
                        let child_unfold_nodes = old_tree
                            .list(&ctx, &store)
                            .await?
                            .map_ok(|(path_element, entry)| match entry {
                                Entry::Tree(child_old_tree_id) => UnfoldNode::Tree {
                                    predecessor: child_old_tree_id,
                                    parent_path_element: Some(path_element),
                                },
                                Entry::Leaf(child_old_leaf_id) => UnfoldNode::Leaf {
                                    predecessor: child_old_leaf_id,
                                    parent_path_element: Some(path_element),
                                },
                            })
                            .try_collect::<Vec<_>>()
                            .await?;

                        Ok((
                            FoldNode::CreateTree {
                                predecessor,
                                parent_path_element,
                            },
                            child_unfold_nodes,
                        ))
                    }
                    UnfoldNode::Leaf {
                        predecessor,
                        parent_path_element,
                    } => Ok((
                        FoldNode::CreateLeaf {
                            predecessor,
                            parent_path_element,
                        },
                        vec![],
                    )),
                }
            }
            .boxed()
        },
        {
            let create_tree = Arc::new(create_tree);
            let create_leaf = Arc::new(create_leaf);
            move |fold_node: FoldNode<OldTreeId, OldLeafId>, subentries| {
                let create_tree = create_tree.clone();
                let create_leaf = create_leaf.clone();
                async move {
                    tokio::spawn(async move {
                        match fold_node {
                            FoldNode::CreateTree {
                                predecessor,
                                parent_path_element,
                            } => {
                                let subentries = subentries.flat_map(
                                    |(path_element, (ctx, entry)): (Option<MPathElement>, _)| {
                                        path_element
                                            .map(|path_element| (path_element, (ctx, entry)))
                                    },
                                );
                                let (ctx, new_tree_id) = create_tree(FromPredecessorTreeInfo {
                                    predecessor,
                                    subentries: subentries.collect(),
                                })
                                .await?;
                                anyhow::Ok((parent_path_element, (ctx, Entry::Tree(new_tree_id))))
                            }
                            FoldNode::CreateLeaf {
                                predecessor,
                                parent_path_element,
                            } => {
                                let (ctx, new_leaf_id) =
                                    create_leaf(FromPredecessorLeafInfo { predecessor }).await?;
                                anyhow::Ok((parent_path_element, (ctx, Entry::Leaf(new_leaf_id))))
                            }
                        }
                    })
                    .await?
                }
                .boxed()
            }
        },
    )
    .await?;

    match entry {
        Entry::Tree(new_tree_id) => Ok(new_tree_id),
        Entry::Leaf(_) => Err(anyhow!(
            "Output of derive_from_predecessor should be a tree"
        )),
    }
}
