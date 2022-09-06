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

    /// The changeset's depth in the skip tree.
    pub skip_tree_depth: u64,
}

/// The parents of a changeset node.
///
/// This uses a smallvec, as there is usually exactly one.  Unlike
/// `ChangesetParents`, this includes the generation number of the parents.
pub type ChangesetNodeParents = SmallVec<[ChangesetNode; 1]>;

/// Outgoing edges from a changeset node.
#[derive(Clone, Debug)]
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

/// A frontier of changesets ordered by generation number.
pub type ChangesetFrontier = BTreeMap<Generation, HashSet<ChangesetId>>;
