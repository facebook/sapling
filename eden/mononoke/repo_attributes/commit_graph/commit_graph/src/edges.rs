/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commit Graph Edges

use std::collections::BTreeMap;
use std::collections::HashSet;

use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use smallvec::SmallVec;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChangesetNode {
    /// The id of the changeset.
    pub cs_id: ChangesetId,

    /// The changeset's generation: the inclusive number of commits between
    /// this commit and the farthest root commit.  Root commits have a
    /// generation of 1.
    pub generation: Generation,
}

/// The parents of a changeset node.
///
/// This uses a smallvec, as there is usually exactly one.  Unlike
/// `ChangesetParents`, this includes the generation number of the parents.
pub type ChangesetNodeParents = SmallVec<[ChangesetNode; 1]>;

/// The merge ancestor or skip tree parent of a commit.  These are combined
/// into a single field to save space: single-parent commits only ever have
/// merge ancestors, and merge commits only ever have skip tree parents.
#[derive(Copy, Clone, Eq, PartialEq, Default)]
pub enum MergeAncestorOrSkipTreeParent {
    #[default]
    None,
    MergeAncestor(ChangesetNode),
    SkipTreeParent(ChangesetNode),
}

impl MergeAncestorOrSkipTreeParent {
    pub fn changeset_node(self) -> Option<ChangesetNode> {
        match self {
            Self::None => None,
            Self::MergeAncestor(node) | Self::SkipTreeParent(node) => Some(node),
        }
    }

    pub fn merge_ancestor(self) -> Option<ChangesetNode> {
        match self {
            Self::None | Self::SkipTreeParent(_) => None,
            Self::MergeAncestor(node) => Some(node),
        }
    }

    pub fn skip_tree_parent(self) -> Option<ChangesetNode> {
        match self {
            Self::None | Self::MergeAncestor(_) => None,
            Self::SkipTreeParent(node) => Some(node),
        }
    }
}

/// Outgoing edges from a changeset node.
#[derive(Clone)]
pub struct ChangesetEdges {
    /// The starting changeset for this set of edges.
    pub node: ChangesetNode,

    /// The changeset's immediate parents.
    pub parents: ChangesetNodeParents,

    /// For root commits, this is `None`.
    ///
    /// For single-parent commits, this is the merge ancestor: the most recent
    /// ancestor that is a merge or root.
    ///
    /// For merge commits, this is the skip tree parent: the single common
    /// ancestor of the commit's parents, if such an ancestor exists.
    pub merge_ancestor_or_skip_tree_parent: MergeAncestorOrSkipTreeParent,

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

/// A frontier of changesets ordered by generation number.
pub type ChangesetFrontier = BTreeMap<Generation, HashSet<ChangesetId>>;
