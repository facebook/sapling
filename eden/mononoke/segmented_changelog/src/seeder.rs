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

use bulkops::ChangesetBulkFetch;
use changesets::ChangesetEntry;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::bundle::SqlBundleStore;
use crate::dag::{Dag, StartState};
use crate::iddag::{IdDagSaveStore, SqlIdDagVersionStore};
use crate::idmap::{IdMap, SqlIdMapVersionStore};
use crate::types::{DagBundle, IdMapVersion};

define_stats! {
    prefix = "mononoke.segmented_changelog.seeder";
    build_all_graph: timeseries(Sum),
}

pub struct SegmentedChangelogSeeder {
    idmap: Arc<dyn IdMap>,
    idmap_version: IdMapVersion,
    idmap_version_store: SqlIdMapVersionStore,
    iddag_version_store: SqlIdDagVersionStore,
    iddag_save_store: IdDagSaveStore,
    bundle_store: SqlBundleStore,
    changeset_bulk_fetch: Arc<dyn ChangesetBulkFetch>,
}

impl SegmentedChangelogSeeder {
    pub fn new(
        idmap: Arc<dyn IdMap>,
        idmap_version: IdMapVersion,
        idmap_version_store: SqlIdMapVersionStore,
        iddag_version_store: SqlIdDagVersionStore,
        iddag_save_store: IdDagSaveStore,
        bundle_store: SqlBundleStore,
        changeset_bulk_fetch: Arc<dyn ChangesetBulkFetch>,
    ) -> Self {
        Self {
            idmap,
            idmap_version,
            idmap_version_store,
            iddag_version_store,
            iddag_save_store,
            bundle_store,
            changeset_bulk_fetch,
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
        // IdDagVersion
        let iddag_version = self
            .iddag_version_store
            .new_version(&ctx, self.idmap_version)
            .await
            .context("fetching new iddag version")?;
        // Save the IdDag
        self.iddag_save_store
            .save(&ctx, iddag_version, &dag.iddag)
            .await
            .context("saving iddag")?;
        // Update BundleStore
        self.bundle_store
            .set(&ctx, DagBundle::new(iddag_version, self.idmap_version))
            .await
            .context("updating bundle store")?;
        // Update IdMapVersion
        self.idmap_version_store
            .set(&ctx, self.idmap_version)
            .await
            .context("updating idmap version")?;
        info!(
            ctx.logger(),
            "finished writing dag bundle and updating metadata, iddag version '{}', \
            idmap version '{}'",
            iddag_version,
            self.idmap_version,
        );
        Ok(())
    }

    pub async fn build_dag_from_scratch(
        &self,
        ctx: &CoreContext,
        head: ChangesetId,
    ) -> Result<(Dag, Vertex)> {
        STATS::build_all_graph.add_value(1);

        let changeset_entries: Vec<ChangesetEntry> =
            self.changeset_bulk_fetch.fetch(ctx).try_collect().await?;
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
        let mut dag = Dag::new(InProcessIdDag::new_in_process(), self.idmap.clone());
        let last_vertex = dag.build(ctx, low_vertex, head, start_state).await?;
        Ok((dag, last_vertex))
    }
}
