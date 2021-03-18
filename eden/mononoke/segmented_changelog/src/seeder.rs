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

use crate::iddag::IdDagSaveStore;
use crate::idmap::IdMapFactory;
use crate::idmap::SqlIdMapVersionStore;
use crate::owned::OwnedSegmentedChangelog;
use crate::types::{IdMapVersion, SegmentedChangelogVersion};
use crate::update::{self, StartState};
use crate::version_store::SegmentedChangelogVersionStore;

define_stats! {
    prefix = "mononoke.segmented_changelog.seeder";
    build_all_graph: timeseries(Sum),
}

pub struct SegmentedChangelogSeeder {
    idmap_version: IdMapVersion,
    idmap_version_store: SqlIdMapVersionStore,
    changeset_bulk_fetch: Arc<PublicChangesetBulkFetch>,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
}

impl SegmentedChangelogSeeder {
    pub fn new(
        idmap_version: IdMapVersion,
        idmap_version_store: SqlIdMapVersionStore,
        changeset_bulk_fetch: Arc<PublicChangesetBulkFetch>,
        sc_version_store: SegmentedChangelogVersionStore,
        iddag_save_store: IdDagSaveStore,
        idmap_factory: IdMapFactory,
    ) -> Self {
        Self {
            idmap_version,
            idmap_version_store,
            changeset_bulk_fetch,
            sc_version_store,
            iddag_save_store,
            idmap_factory,
        }
    }
    pub async fn run(&self, ctx: &CoreContext, head: ChangesetId) -> Result<()> {
        info!(
            ctx.logger(),
            "seeding segmented changelog using idmap version: {}", self.idmap_version
        );
        let (owned, last_vertex) = self
            .build_from_scratch(&ctx, head)
            .await
            .context("building dag from scratch")?;
        info!(
            ctx.logger(),
            "finished building dag, head '{}' has assigned vertex '{}'", head, last_vertex
        );
        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(&ctx, &owned.iddag)
            .await
            .context("error saving iddag")?;
        // Update SegmentedChangelogVersion
        let sc_version = SegmentedChangelogVersion::new(iddag_version, self.idmap_version);
        self.sc_version_store
            .set(&ctx, sc_version)
            .await
            .context("error updating segmented changelog version store")?;
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

    pub async fn build_from_scratch(
        &self,
        ctx: &CoreContext,
        head: ChangesetId,
    ) -> Result<(OwnedSegmentedChangelog, Vertex)> {
        STATS::build_all_graph.add_value(1);

        let changeset_entries: Vec<ChangesetEntry> = self
            .changeset_bulk_fetch
            .fetch(ctx, Direction::OldestFirst)
            .inspect_ok({
                let mut count = 1;
                move |_| {
                    count += 1;
                    if count % 100000 == 0 {
                        info!(ctx.logger(), "{} changesets loaded ", count);
                    }
                }
            })
            .try_collect()
            .await?;
        info!(
            ctx.logger(),
            "{} changesets loaded",
            changeset_entries.len()
        );
        let mut start_state = StartState::new();
        for cs_entry in changeset_entries.into_iter() {
            start_state.insert_parents(cs_entry.cs_id, cs_entry.parents);
        }

        let low_vertex = dag::Group::MASTER.min_id();
        let idmap = self.idmap_factory.for_writer(ctx, self.idmap_version);
        let mut iddag = InProcessIdDag::new_in_process();

        let (mem_idmap, head_vertex) = update::assign_ids(ctx, &start_state, head, low_vertex)?;


        update::update_iddag(ctx, &mut iddag, &start_state, &mem_idmap, head_vertex)?;
        update::update_idmap(ctx, &idmap, &mem_idmap).await?;

        let owned = OwnedSegmentedChangelog::new(iddag, idmap);
        Ok((owned, head_vertex))
    }
}
