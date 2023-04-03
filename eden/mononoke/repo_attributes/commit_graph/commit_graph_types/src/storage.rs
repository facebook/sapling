/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commit Graph Storage
//!
//! Trait for the storage back-end for the commit graph.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use mononoke_types::FIRST_GENERATION;
use vec1::Vec1;

use crate::edges::ChangesetEdges;

/// Indication of the kind of edge to traverse for prefetch.
#[derive(Copy, Clone, Debug)]
pub enum PrefetchEdge {
    /// Prefetch a linear range of commits by following the first parent
    FirstParent,

    /// Prefetch along the maximum skip tree distance by following the skip
    /// tree skew ancestor, or first parent if the commit does not have
    /// a skip tree skew ancestor
    SkipTreeSkewAncestor,
}

/// Where to prefetch to.
#[derive(Copy, Clone, Debug)]
pub struct PrefetchTarget {
    /// Prefetch along this edge.
    pub edge: PrefetchEdge,

    /// Prefetch as far back as this generation.
    pub generation: Generation,

    /// Prefetch up to this many steps.
    pub steps: u64,
}

/// Indication for additional changesets to be fetched for subsequent
/// traversals.
///
/// If efficient to do so, implementors should use this hint to fetch
/// additional edges that will be useful for skew-binary traversal
/// to the target generation.
#[derive(Copy, Clone, Debug)]
pub enum Prefetch {
    /// No prefetch is required.
    None,

    /// Prefetch is permitted with the given hint, but additional items are
    /// not to be returned.
    Hint(PrefetchTarget),

    /// Prefetch if possible, and included prefetched items in the result.
    Include(PrefetchTarget),
}

impl Prefetch {
    /// Prepare prefetching for skew-binary traversal over the skip tree.
    pub fn for_skip_tree_traversal(generation: Generation) -> Self {
        // We are prefetching mostly along the skew ancestor edge, which
        // should typically be O(log(N)) in length, except that for merge
        // commits without a common ancestor we follow the p1 parent, so limit
        // to 32 steps so that we don't follow the p1 ancestry too far.
        Prefetch::Hint(PrefetchTarget {
            edge: PrefetchEdge::SkipTreeSkewAncestor,
            generation,
            steps: 32,
        })
    }

    /// Prepare prefetching for linear traversal of the p1 history.
    pub fn for_p1_linear_traversal() -> Self {
        // Prefetch linear ranges of 128 commits.  This is arbitrary, but is a
        // balance between not overfetching for the cache and reducing the
        // number of sequential steps.
        Prefetch::Hint(PrefetchTarget {
            edge: PrefetchEdge::FirstParent,
            generation: FIRST_GENERATION,
            steps: 128,
        })
    }

    pub fn is_hint(&self) -> bool {
        matches!(self, Prefetch::Hint(..))
    }

    pub fn is_include(&self) -> bool {
        matches!(self, Prefetch::Include(..))
    }

    pub fn include_hint(self) -> Prefetch {
        match self {
            Prefetch::None => Prefetch::None,
            Prefetch::Hint(target) | Prefetch::Include(target) => Prefetch::Include(target),
        }
    }

    pub fn target(self) -> Option<PrefetchTarget> {
        match self {
            Prefetch::None => None,
            Prefetch::Hint(target) | Prefetch::Include(target) => Some(target),
        }
    }
}

/// Commit Graph Storage.
#[async_trait]
pub trait CommitGraphStorage: Send + Sync {
    /// The repository this commit graph storage is for.
    fn repo_id(&self) -> RepositoryId;

    /// Add a new changeset to the commit graph.
    ///
    /// Returns true if a new changeset was inserted, or false if the
    /// changeset already existed.
    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool>;

    /// Add many changesets at once. Used for low level stuff like backfilling.
    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize>;

    /// Returns the changeset graph edges for this changeset.
    async fn fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>>;

    /// Returns the changeset graph edges for this changeset, or an error of
    /// this changeset is missing in the commit graph.
    async fn fetch_edges_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetEdges>;

    /// Returns the changeset graph edges for multiple changesets.
    ///
    /// Prefetch indicates that this request is part of a larger request
    /// involving commits down to a particular generation number, and so
    /// prefetching more nodes into any internal caches would be beneficial.
    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>>;

    /// Same as fetch_many_edges but returns an error if any of
    /// the changesets are missing in the commit graph.
    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>>;

    /// Find all changeset ids with a given prefix.
    async fn find_by_prefix(
        &self,
        _ctx: &CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix>;
}
