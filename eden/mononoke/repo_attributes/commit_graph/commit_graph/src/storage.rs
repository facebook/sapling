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

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use vec1::Vec1;

use crate::edges::ChangesetEdges;

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
    ) -> Result<ChangesetEdges> {
        self.fetch_edges(ctx, cs_id)
            .await?
            .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))
    }

    /// Returns the changeset graph edges for multiple changesets.
    ///
    /// Prefetch hint indicates that this request is part of a larger request
    /// involving commits down to a particular generation number, and so
    /// prefetching more nodes into any internal caches would be beneficial.
    ///
    /// If prefetching does occur, it is internal to the caches, and this
    /// method will only return edges for the requested changesets.
    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        _prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>>;

    /// Same as fetch_many_edges but returns an error if any of
    /// the changesets are missing in the commit graph.
    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let edges = self.fetch_many_edges(ctx, cs_ids, prefetch_hint).await?;
        let missing_changesets: Vec<_> = cs_ids
            .iter()
            .filter(|cs_id| !edges.contains_key(cs_id))
            .collect();

        if !missing_changesets.is_empty() {
            Err(anyhow!(
                "Missing changesets in commit graph: {}",
                missing_changesets
                    .into_iter()
                    .map(|cs_id| format!("{}, ", cs_id))
                    .collect::<String>()
            ))
        } else {
            Ok(edges)
        }
    }

    /// Returns the changeset graph edges for multiple changesets plus
    /// additional prefetched edges for subsequent traversals.
    ///
    /// If possible, implementors of this method should additionally fetch
    /// more ancestors down to the prefetch hint, and include these prefetched
    /// edges in the return value.
    async fn fetch_many_edges_with_prefetch(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Generation,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        self.fetch_many_edges(ctx, cs_ids, Some(prefetch_hint))
            .await
    }

    /// Find all changeset ids with a given prefix.
    async fn find_by_prefix(
        &self,
        _ctx: &CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix>;
}
