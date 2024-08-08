/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use commit_graph::BaseCommitGraphWriter;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_graph::ParentsFetcher;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::FetchedChangesetEdges;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use memwrites_commit_graph_storage::MemWritesCommitGraphStorage;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use smallvec::ToSmallVec;
use sql::Connection;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;
use vec1::vec1;
use vec1::Vec1;

use crate::bubble::BubbleId;

mononoke_queries! {
    read SelectChangesets(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        >list cs_id: ChangesetId
    ) -> (ChangesetId) {
        "SELECT cs_id
          FROM ephemeral_bubble_changeset_mapping
          WHERE repo_id = {repo_id} AND bubble_id = {bubble_id} AND cs_id IN {cs_id}"
    }

    read SelectChangesetsInRange(repo_id: RepositoryId, min_id: ChangesetId, max_id: ChangesetId, limit: usize) -> (ChangesetId) {
        "
        SELECT cs_id
        FROM ephemeral_bubble_changeset_mapping
        WHERE repo_id = {repo_id} AND {min_id} <= cs_id AND cs_id <= {max_id}
        ORDER BY cs_id ASC
        LIMIT {limit}
        "
    }

    write InsertChangeset(
        values: (repo_id: RepositoryId, cs_id: ChangesetId, bubble_id: BubbleId, gen: u64)
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO ephemeral_bubble_changeset_mapping
         (repo_id, cs_id, bubble_id, gen)
         VALUES {values}"
    }
}

/// A commit graph storage that allows fetching snapshot changesets, as well
/// as presistent changesets.
/// Since initially there will be a single snapshot per bubble, there's no
/// need to optimise anything on this struct. As the need arises, we can tweak
/// this, for example by having an extra table that stores parent information
/// to avoid looking at the blobstore.
#[derive(Clone)]
pub struct EphemeralCommitGraphStorage {
    /// Storage containing only the ephemeral bubble changesets.
    ephemeral_only_storage: Arc<EphemeralOnlyChangesetStorage>,
    /// A storage that wraps the persistent commit graph storage and writes
    /// new changesets to memory.
    mem_writes_storage: Arc<MemWritesCommitGraphStorage>,
    /// Another view of the MemWrites storage to allow traversing the commit graph
    /// (through CommitGraphWriter::add_recursive) to create ChangesetEdges.
    mem_writes_commit_graph_writer: BaseCommitGraphWriter,
}

/// A storage that allows fetching snapshot changesets.
#[derive(Clone)]
pub struct EphemeralOnlyChangesetStorage {
    repo_id: RepositoryId,
    bubble_id: BubbleId,
    repo_blobstore: RepoBlobstore,
    connections: SqlConnections,
}

impl EphemeralCommitGraphStorage {
    pub(crate) fn new(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        repo_blobstore: RepoBlobstore,
        connections: SqlConnections,
        persistent_storage: Arc<dyn CommitGraphStorage>,
    ) -> Self {
        let mem_writes_storage = Arc::new(MemWritesCommitGraphStorage::new(persistent_storage));
        Self {
            ephemeral_only_storage: Arc::new(EphemeralOnlyChangesetStorage::new(
                repo_id,
                bubble_id,
                repo_blobstore,
                connections,
            )),
            mem_writes_storage: mem_writes_storage.clone(),
            mem_writes_commit_graph_writer: BaseCommitGraphWriter::new(CommitGraph::new(
                mem_writes_storage,
            )),
        }
    }
}

impl EphemeralOnlyChangesetStorage {
    pub(crate) fn new(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        repo_blobstore: RepoBlobstore,
        connections: SqlConnections,
    ) -> Self {
        Self {
            repo_id,
            bubble_id,
            repo_blobstore,
            connections,
        }
    }

    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, _ctx: &CoreContext, edges: &ChangesetEdges) -> Result<bool> {
        let result = InsertChangeset::query(
            &self.connections.write_connection,
            &[(
                &self.repo_id,
                &edges.node.cs_id,
                &self.bubble_id,
                &edges.node.generation.value(),
            )],
        )
        .await?;

        Ok(result.last_insert_id().is_some())
    }

    pub async fn known_changesets(
        &self,
        _ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        let found_cs_ids = SelectChangesets::query(
            &self.connections.read_connection,
            &self.repo_id,
            &self.bubble_id,
            &cs_ids,
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect::<HashSet<_>>();

        let (mut known_ids, missing_ids): (Vec<_>, Vec<_>) = cs_ids
            .into_iter()
            .partition(|cs_id| found_cs_ids.contains(cs_id));

        if !missing_ids.is_empty() {
            let found_in_master = SelectChangesets::query(
                &self.connections.read_master_connection,
                &self.repo_id,
                &self.bubble_id,
                &missing_ids,
            )
            .await?
            .into_iter()
            .map(|(cs_id,)| cs_id);

            known_ids.extend(found_in_master);
        }

        Ok(known_ids)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        let fetched_ids = SelectChangesetsInRange::maybe_traced_query(
            &self.connections.read_connection,
            ctx.client_request_info(),
            &self.repo_id,
            &cs_prefix.min_bound(),
            &cs_prefix.max_bound(),
            &(limit + 1),
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect::<Vec<_>>();

        Ok(ChangesetIdsResolvedFromPrefix::from_vec_and_limit(
            fetched_ids,
            limit,
        ))
    }
}

#[async_trait]
impl ParentsFetcher for EphemeralOnlyChangesetStorage {
    async fn fetch_parents(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        let cs = cs_id
            .load(ctx, &self.repo_blobstore)
            .await
            .with_context(|| {
                format!(
                    "Failed to load bonsai changeset with id {} (in EphemeralCommitGraphStorage)",
                    cs_id
                )
            })?;
        Ok(cs.parents().collect())
    }
}

#[async_trait]
impl CommitGraphStorage for EphemeralCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.ephemeral_only_storage.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        let modified = self.ephemeral_only_storage.add(ctx, &edges).await?;
        self.mem_writes_commit_graph_writer
            .add_recursive(
                ctx,
                self.ephemeral_only_storage.clone(),
                vec1![(
                    edges.node.cs_id,
                    edges.parents.into_iter().map(|node| node.cs_id).collect(),
                )],
            )
            .await?;

        Ok(modified)
    }

    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize> {
        let mut modified = 0;
        for edges in many_edges {
            modified += self.add(ctx, edges).await? as usize;
        }
        Ok(modified)
    }

    async fn fetch_edges(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<ChangesetEdges> {
        Ok(self
            .fetch_many_edges(ctx, &[cs_id], Prefetch::None)
            .await?
            .remove(&cs_id)
            .ok_or_else(|| {
                anyhow!(
                    "Missing changeset {} in ephemeral commit graph storage",
                    cs_id
                )
            })?
            .into())
    }

    async fn maybe_fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        Ok(self
            .fetch_many_edges(ctx, &[cs_id], Prefetch::None)
            .await?
            .remove(&cs_id)
            .map(|edges| edges.into()))
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        let fetched_edges = self.maybe_fetch_many_edges(ctx, cs_ids, prefetch).await?;

        let unfetched_ids = cs_ids
            .iter()
            .filter(|cs_id| !fetched_edges.contains_key(cs_id))
            .copied()
            .collect::<Vec<_>>();

        if let Some(cs_id) = unfetched_ids.first() {
            return Err(anyhow::anyhow!(
                "Changeset {} not found in ephemeral commit graph storage",
                cs_id,
            ));
        }

        Ok(fetched_edges)
    }

    async fn maybe_fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        let mut fetched_edges = self
            .mem_writes_storage
            .maybe_fetch_many_edges(ctx, cs_ids, prefetch)
            .await?;

        let unfetched_ids = cs_ids
            .iter()
            .filter(|cs_id| !fetched_edges.contains_key(cs_id))
            .copied()
            .collect::<Vec<_>>();

        if !unfetched_ids.is_empty() {
            let known_ids = self
                .ephemeral_only_storage
                .known_changesets(ctx, unfetched_ids)
                .await?;

            for cs_id in known_ids {
                let parents = self
                    .ephemeral_only_storage
                    .fetch_parents(ctx, cs_id)
                    .await?;
                self.mem_writes_commit_graph_writer
                    .add_recursive(
                        ctx,
                        self.ephemeral_only_storage.clone(),
                        vec1![(cs_id, parents.to_smallvec(),)],
                    )
                    .await?;
                fetched_edges.insert(
                    cs_id,
                    self.mem_writes_storage
                        .fetch_edges(ctx, cs_id)
                        .await?
                        .into(),
                );
            }
        }

        Ok(fetched_edges)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        match futures::try_join!(
            self.ephemeral_only_storage
                .find_by_prefix(ctx, cs_prefix, limit),
            self.mem_writes_storage
                .find_by_prefix(ctx, cs_prefix, limit)
        )? {
            (ephemeral_only_matches @ ChangesetIdsResolvedFromPrefix::TooMany(_), _) => {
                Ok(ephemeral_only_matches)
            }
            (_, mem_writes_matches @ ChangesetIdsResolvedFromPrefix::TooMany(_)) => {
                Ok(mem_writes_matches)
            }
            (ephemeral_only_matches, mem_writes_matches) => {
                Ok(ChangesetIdsResolvedFromPrefix::from_vec_and_limit(
                    ephemeral_only_matches
                        .to_vec()
                        .into_iter()
                        .chain(mem_writes_matches.to_vec())
                        .collect::<HashSet<_>>()
                        .into_iter()
                        .collect(),
                    limit,
                ))
            }
        }
    }

    async fn fetch_children(
        &self,
        _ctx: &CoreContext,
        _cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        // Implementing this requires storing parent information in the snapshot SQL tables, but
        // there's no need for it currently as we don't support stacks of ephemeral changesets.
        unimplemented!("Fetching changeset children is not implemented for ephemeral commit graph")
    }
}
