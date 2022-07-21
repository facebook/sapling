/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use context::CoreContext;
use derivative::Derivative;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use sorted_vector_map::SortedVectorMap;
use sql::queries;
use sql::Connection;
use sql_ext::SqlConnections;

use std::sync::Arc;

use crate::bubble::BubbleId;

// Knows how to fetch snapshot changesets. Since initially there will
// be a single snapshot per bubble, there's no need to optimise anything
// on this class. As the need arises, we can tweak this, for example by
// having an extra table that stores parent information to avoid looking
// at the blobstore.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct EphemeralChangesets {
    repo_id: RepositoryId,
    bubble_id: BubbleId,
    repo_blobstore: RepoBlobstore,
    #[derivative(Debug = "ignore")]
    connections: SqlConnections,
    #[derivative(Debug = "ignore")]
    persistent_changesets: Arc<dyn Changesets>,
}

queries! {
    read SelectChangesets(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        >list cs_id: ChangesetId
    ) -> (ChangesetId, u64) {
        "SELECT cs_id, gen
         FROM ephemeral_bubble_changeset_mapping
         WHERE repo_id = {repo_id} AND bubble_id = {bubble_id} AND cs_id IN {cs_id}"
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

impl EphemeralChangesets {
    pub(crate) fn new(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        repo_blobstore: RepoBlobstore,
        connections: SqlConnections,
        persistent_changesets: Arc<dyn Changesets>,
    ) -> Self {
        Self {
            repo_id,
            bubble_id,
            repo_blobstore,
            connections,
            persistent_changesets,
        }
    }

    async fn fetch_gens_with_connection(
        &self,
        cs_ids: &[ChangesetId],
        connection: &Connection,
    ) -> Result<Vec<(ChangesetId, u64)>> {
        SelectChangesets::query(connection, &self.repo_id, &self.bubble_id, cs_ids).await
    }

    pub async fn fetch_gens(
        &self,
        cs_ids: &[ChangesetId],
    ) -> Result<SortedVectorMap<ChangesetId, u64>> {
        let mut gens: SortedVectorMap<_, _> = self
            .fetch_gens_with_connection(cs_ids, &self.connections.read_connection)
            .await?
            .into_iter()
            .collect();
        if gens.len() != cs_ids.len() {
            let missing: Vec<_> = cs_ids
                .iter()
                .cloned()
                .filter(|id| !gens.contains_key(id))
                .collect();
            let mut extra = self
                .fetch_gens_with_connection(
                    missing.as_slice(),
                    &self.connections.read_master_connection,
                )
                .await?
                .into_iter()
                .collect();
            gens.append(&mut extra);
        }
        Ok(gens)
    }

    async fn get_ephemeral(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
    ) -> Result<Vec<ChangesetEntry>> {
        let gens = self.fetch_gens(cs_ids).await?;
        let changesets: Vec<_> = stream::iter(
            cs_ids
                .iter()
                .filter(|id| gens.get(id).is_some())
                .map(|id| id.load(ctx, &self.repo_blobstore))
                .collect::<Vec<_>>(), // without this we get compile errors
        )
        .buffered(100)
        .try_collect()
        .await?;
        Ok(changesets
            .into_iter()
            .filter_map(|cs| {
                let cs_id = cs.get_changeset_id();
                gens.get(&cs_id).map(|gen| ChangesetEntry {
                    repo_id: self.repo_id,
                    cs_id,
                    parents: cs.parents().collect(),
                    gen: *gen,
                })
            })
            .collect())
    }
}

#[async_trait]
impl Changesets for EphemeralChangesets {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> Result<bool> {
        let parents_len = cs.parents.len();
        let parents = self.get_many(ctx, cs.parents.clone()).await?;
        if parents.len() != parents_len {
            bail!(
                "Not all parents found, expected [{}], found [{}]",
                cs.parents.into_iter().map(|id| id.to_string()).join(", "),
                parents
                    .into_iter()
                    .map(|entry| entry.cs_id.to_string())
                    .join(", ")
            );
        }
        let gen = parents
            .into_iter()
            .map(|entry| entry.gen)
            .max()
            .unwrap_or(0)
            + 1;
        let result = InsertChangeset::query(
            &self.connections.write_connection,
            &[(&self.repo_id, &cs.cs_id, &self.bubble_id, &gen)],
        )
        .await?;
        Ok(result.last_insert_id().is_some())
    }

    async fn get(&self, ctx: CoreContext, cs_id: ChangesetId) -> Result<Option<ChangesetEntry>> {
        Ok(self.get_many(ctx, vec![cs_id]).await?.into_iter().next())
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>> {
        let ephemeral = self.get_ephemeral(&ctx, &cs_ids);
        let persistent = self
            .persistent_changesets
            .get_many(ctx.clone(), cs_ids.clone());
        let (mut ephemeral, persistent) = try_join!(ephemeral, persistent)?;
        ephemeral.extend(persistent);
        Ok(ephemeral)
    }

    /// Use caching for the full changeset ids and slower path otherwise.
    async fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        unimplemented!()
    }

    fn prime_cache(&self, _ctx: &CoreContext, _changesets: &[ChangesetEntry]) {
        // no caching involved
    }

    async fn enumeration_bounds(
        &self,
        _ctx: &CoreContext,
        _read_from_master: bool,
        _known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>> {
        unimplemented!()
    }

    fn list_enumeration_range(
        &self,
        _ctx: &CoreContext,
        _min_id: u64,
        _max_id: u64,
        _sort_and_limit: Option<(SortOrder, u64)>,
        _read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64)>> {
        unimplemented!()
    }
}
