/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commit Graph Edges

use std::num::NonZeroU32;

use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Result;
use commit_graph_thrift as thrift;
use futures::Future;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use smallvec::SmallVec;

#[derive(Abomonation, Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChangesetNode {
    /// The id of the changeset.
    pub cs_id: ChangesetId,

    /// The changeset's generation: the inclusive number of commits between
    /// this commit and the farthest root commit.  Root commits have a
    /// generation of 1.
    pub generation: Generation,

    /// The changeset's depth in the skip tree.
    pub skip_tree_depth: u64,

    /// The changeset's depth in the p1 tree
    pub p1_linear_depth: u64,
}

impl ChangesetNode {
    pub fn to_thrift(&self) -> thrift::ChangesetNode {
        thrift::ChangesetNode {
            cs_id: self.cs_id.into_thrift(),
            generation: thrift::Generation(self.generation.value() as i64),
            skip_tree_depth: self.skip_tree_depth as i64,
            p1_linear_depth: self.p1_linear_depth as i64,
        }
    }

    pub fn from_thrift(node: thrift::ChangesetNode) -> Result<Self> {
        Ok(Self {
            cs_id: ChangesetId::from_thrift(node.cs_id)?,
            generation: Generation::new(node.generation.0 as u64),
            skip_tree_depth: node.skip_tree_depth as u64,
            p1_linear_depth: node.p1_linear_depth as u64,
        })
    }
}
/// The parents of a changeset.
///
/// This uses a smallvec, as there is usually exactly one.
pub type ChangesetParents = SmallVec<[ChangesetId; 1]>;

/// The parents of a changeset node.
///
/// This uses a smallvec, as there is usually exactly one.  Unlike
/// `ChangesetParents`, this includes the generation number of the parents.
pub type ChangesetNodeParents = SmallVec<[ChangesetNode; 1]>;

/// Outgoing edges from a changeset node.
#[derive(Abomonation, Clone, Debug, Eq, PartialEq)]
pub struct ChangesetEdges {
    /// The starting changeset for this set of edges.
    pub node: ChangesetNode,

    /// The changeset's immediate parents.
    pub parents: ChangesetNodeParents,

    /// For root and merge commits, this is `None`.
    ///
    /// For single-parent commits, this is the merge ancestor: the most recent
    /// ancestor that is a merge or root.
    pub merge_ancestor: Option<ChangesetNode>,

    /// The skip tree parent: this is the most recent single common ancestor
    /// of this commit's parents
    pub skip_tree_parent: Option<ChangesetNode>,

    /// The skip tree skew ancestor: this is some ancestor of the common
    /// ancestors of this commit's parents, which provides a skew-binary
    /// search tree over the commit graph, if such an ancestor exists.
    pub skip_tree_skew_ancestor: Option<ChangesetNode>,

    /// The p1-linear skew ancestor: this is some ancestor of the first
    /// parent of this commit, which provides a skew-binary search tree over
    /// the linear first-parent history of this commit, if such an ancestor
    /// exists.
    ///
    /// Note: this excludes any merged in branches, so should only be used for
    /// history-lossy operations.
    pub p1_linear_skew_ancestor: Option<ChangesetNode>,
}

impl ChangesetEdges {
    pub fn to_thrift(&self) -> thrift::ChangesetEdges {
        thrift::ChangesetEdges {
            node: self.node.to_thrift(),
            parents: self.parents.iter().map(ChangesetNode::to_thrift).collect(),
            merge_ancestor: self.merge_ancestor.as_ref().map(ChangesetNode::to_thrift),
            skip_tree_parent: self.skip_tree_parent.as_ref().map(ChangesetNode::to_thrift),
            skip_tree_skew_ancestor: self
                .skip_tree_skew_ancestor
                .as_ref()
                .map(ChangesetNode::to_thrift),
            p1_linear_skew_ancestor: self
                .p1_linear_skew_ancestor
                .as_ref()
                .map(ChangesetNode::to_thrift),
        }
    }

    pub fn from_thrift(edges: thrift::ChangesetEdges) -> Result<Self> {
        Ok(Self {
            node: ChangesetNode::from_thrift(edges.node)?,
            parents: edges
                .parents
                .into_iter()
                .map(ChangesetNode::from_thrift)
                .collect::<Result<ChangesetNodeParents>>()?,
            merge_ancestor: edges
                .merge_ancestor
                .map(ChangesetNode::from_thrift)
                .transpose()?,
            skip_tree_parent: edges
                .skip_tree_parent
                .map(ChangesetNode::from_thrift)
                .transpose()?,
            skip_tree_skew_ancestor: edges
                .skip_tree_skew_ancestor
                .map(ChangesetNode::from_thrift)
                .transpose()?,
            p1_linear_skew_ancestor: edges
                .p1_linear_skew_ancestor
                .map(ChangesetNode::from_thrift)
                .transpose()?,
        })
    }

    /// Returns the lowest skip tree edge (skip_tree_parent or skip_tree_skew_ancestor)
    /// that satisfies the given property, or None if neither does.
    pub async fn lowest_skip_tree_edge_with<Property, Out>(
        &self,
        property: Property,
    ) -> Result<Option<ChangesetNode>>
    where
        Property: Fn(ChangesetNode) -> Out,
        Out: Future<Output = Result<bool>>,
    {
        if let Some(skip_tree_skew_ancestor) = self.skip_tree_skew_ancestor {
            if property(skip_tree_skew_ancestor).await? {
                return Ok(Some(skip_tree_skew_ancestor));
            }
        }

        if let Some(skip_tree_parent) = self.skip_tree_parent {
            if property(skip_tree_parent).await? {
                return Ok(Some(skip_tree_parent));
            }
        }

        Ok(None)
    }
}

/// A smaller version of ChangesetEdges for use in cases where
/// space efficiency matters (e.g. preloading the commit graph).
///
/// Outgoing edges are replaced by u32 ids identifying a changeset.
#[derive(Debug)]
pub struct CompactChangesetEdges {
    pub generation: u32,
    pub skip_tree_depth: u32,
    pub p1_linear_depth: u32,
    pub parents: SmallVec<[NonZeroU32; 2]>,
    pub merge_ancestor: Option<NonZeroU32>,
    pub skip_tree_parent: Option<NonZeroU32>,
    pub skip_tree_skew_ancestor: Option<NonZeroU32>,
    pub p1_linear_skew_ancestor: Option<NonZeroU32>,
}

impl CompactChangesetEdges {
    pub fn to_thrift(
        &self,
        cs_id: ChangesetId,
        unique_id: NonZeroU32,
    ) -> thrift::CompactChangesetEdges {
        thrift::CompactChangesetEdges {
            cs_id: cs_id.into_thrift(),
            unique_id: unique_id.get() as i32,
            generation: self.generation as i32,
            skip_tree_depth: self.skip_tree_depth as i32,
            p1_linear_depth: self.p1_linear_depth as i32,
            parents: self
                .parents
                .iter()
                .copied()
                .map(|id| id.get() as i32)
                .collect(),
            merge_ancestor: self.merge_ancestor.map(|id| id.get() as i32),
            skip_tree_parent: self.skip_tree_parent.map(|id| id.get() as i32),
            skip_tree_skew_ancestor: self.skip_tree_skew_ancestor.map(|id| id.get() as i32),
            p1_linear_skew_ancestor: self.p1_linear_skew_ancestor.map(|id| id.get() as i32),
        }
    }

    pub fn from_thrift(edges: thrift::CompactChangesetEdges) -> Result<Self> {
        Ok(Self {
            generation: edges.generation as u32,
            skip_tree_depth: edges.skip_tree_depth as u32,
            p1_linear_depth: edges.p1_linear_depth as u32,
            parents: edges
                .parents
                .into_iter()
                .map(|id| {
                    NonZeroU32::new(id as u32)
                        .ok_or_else(|| anyhow!("Couldn't convert parent id to NonZeroU32"))
                })
                .collect::<Result<_>>()?,
            merge_ancestor: edges
                .merge_ancestor
                .map(|id| {
                    NonZeroU32::new(id as u32)
                        .ok_or_else(|| anyhow!("Couldn't convert merge ancestor id to NonZeroU32"))
                })
                .transpose()?,
            skip_tree_parent: edges
                .skip_tree_parent
                .map(|id| {
                    NonZeroU32::new(id as u32).ok_or_else(|| {
                        anyhow!("Couldn't convert skip tree parent id to NonZeroU32")
                    })
                })
                .transpose()?,
            skip_tree_skew_ancestor: edges
                .skip_tree_skew_ancestor
                .map(|id| {
                    NonZeroU32::new(id as u32).ok_or_else(|| {
                        anyhow!("Couldn't convert skip tree skew ancestor id to NonZeroU32")
                    })
                })
                .transpose()?,
            p1_linear_skew_ancestor: edges
                .p1_linear_skew_ancestor
                .map(|id| {
                    NonZeroU32::new(id as u32).ok_or_else(|| {
                        anyhow!("Couldn't convert p1 linear skew ancestor id to NonZeroU32")
                    })
                })
                .transpose()?,
        })
    }
}
