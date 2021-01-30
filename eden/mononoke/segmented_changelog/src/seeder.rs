/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::TryStreamExt;
use slog::info;

use dag::{self, Id as Vertex, InProcessIdDag};
use stats::prelude::*;

use bulkops::{Direction, PublicChangesetBulkFetch};
use changesets::ChangesetEntry;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::{Dag, StartState};
use crate::idmap::SqlIdMapVersionStore;
use crate::manager::SegmentedChangelogManager;
use crate::types::IdMapVersion;

define_stats! {
    prefix = "mononoke.segmented_changelog.seeder";
    build_all_graph: timeseries(Sum),
}

pub struct SegmentedChangelogSeeder {
    idmap_version: IdMapVersion,
    idmap_version_store: SqlIdMapVersionStore,
    changeset_bulk_fetch: Arc<PublicChangesetBulkFetch>,
    manager: SegmentedChangelogManager,
}

impl SegmentedChangelogSeeder {
    pub fn new(
        idmap_version: IdMapVersion,
        idmap_version_store: SqlIdMapVersionStore,
        changeset_bulk_fetch: Arc<PublicChangesetBulkFetch>,
        manager: SegmentedChangelogManager,
    ) -> Self {
        Self {
            idmap_version,
            idmap_version_store,
            changeset_bulk_fetch,
            manager,
        }
    }
    pub async fn run(&self, ctx: &CoreContext, head: ChangesetId) -> Result<()> {
        info!(
            ctx.logger(),
            "seeding segmented changelog using idmap version: {}", self.idmap_version
        );
        let (dag, last_vertex) = self
            .build_dag_from_scratch(&ctx, head)
            .await
            .context("building dag from scratch")?;
        info!(
            ctx.logger(),
            "finished building dag, head '{}' has assigned vertex '{}'", head, last_vertex
        );
        self.manager
            .save_dag(ctx, &dag.iddag, self.idmap_version)
            .await
            .context("failed to save dag")?;
        // Update IdMapVersion
        self.idmap_version_store
            .set(&ctx, self.idmap_version)
            .await
            .context("updating idmap version")?;
        info!(
            ctx.logger(),
            "successfully finished seeding segmented changelog",
        );
        Ok(())
    }

    pub async fn build_dag_from_scratch(
        &self,
        ctx: &CoreContext,
        head: ChangesetId,
    ) -> Result<(Dag, Vertex)> {
        STATS::build_all_graph.add_value(1);

        let changeset_entries: Vec<ChangesetEntry> = self
            .changeset_bulk_fetch
            .fetch(ctx, Direction::OldestFirst)
            .try_collect()
            .await?;
        info!(
            ctx.logger(),
            "loaded {} changesets",
            changeset_entries.len()
        );
        let mut start_state = StartState::new();
        for cs_entry in changeset_entries.into_iter() {
            start_state.insert_parents(cs_entry.cs_id, cs_entry.parents);
        }

        let low_vertex = dag::Group::MASTER.min_id();
        let idmap = self.manager.new_idmap(self.idmap_version);
        let mut dag = Dag::new(InProcessIdDag::new_in_process(), idmap);
        let last_vertex = dag.build(ctx, low_vertex, head, start_state).await?;
        Ok((dag, last_vertex))
    }
}
