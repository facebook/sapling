/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::sync::RwLock;

use dag::{self, CloneData, Location};
use stats::prelude::*;

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::Dag;
use crate::idmap::IdMap;
use crate::update::build_incremental;
use crate::{SegmentedChangelog, StreamCloneData};

define_stats! {
    prefix = "mononoke.segmented_changelog.ondemand";
    location_to_changeset_id: timeseries(Sum),
    changeset_id_to_location: timeseries(Sum),
}

pub struct OnDemandUpdateDag {
    dag: RwLock<Dag>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
}

impl OnDemandUpdateDag {
    pub fn new(dag: Dag, changeset_fetcher: Arc<dyn ChangesetFetcher>) -> Self {
        Self {
            dag: RwLock::new(dag),
            changeset_fetcher,
        }
    }
}

#[async_trait]
impl SegmentedChangelog for OnDemandUpdateDag {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        {
            let dag = self.dag.read().await;
            if let Some(known_vertex) = dag
                .idmap
                .find_vertex(ctx, location.descendant)
                .await
                .context("fetching vertex for csid")?
            {
                if dag.iddag.contains_id(known_vertex)? {
                    let location = location.with_descendant(known_vertex);
                    return dag
                        .known_location_to_many_changeset_ids(ctx, location, count)
                        .await
                        .context("ondemand first known_location_many_changest_ids");
                }
            }
        }
        let known_vertex = {
            let mut dag = self.dag.write().await;
            build_incremental(ctx, &mut dag, &self.changeset_fetcher, location.descendant).await?
        };
        let dag = self.dag.read().await;
        let location = Location::new(known_vertex, location.distance);
        dag.known_location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        client_head: ChangesetId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
        STATS::changeset_id_to_location.add_value(1);
        {
            let dag = self.dag.read().await;
            if let Some(_vertex) = dag
                .idmap
                .find_vertex(ctx, client_head)
                .await
                .context("fetching vertex for csid")?
            {
                return dag
                    .many_changeset_ids_to_locations(ctx, client_head, cs_ids)
                    .await
                    .context("failed ondemand read many_changeset_ids_to_locations");
            }
        }
        {
            let mut dag = self.dag.write().await;
            build_incremental(ctx, &mut dag, &self.changeset_fetcher, client_head).await?
        };
        let dag = self.dag.read().await;
        return dag
            .many_changeset_ids_to_locations(ctx, client_head, cs_ids)
            .await
            .context("failed ondemand read many_changeset_ids_to_locations");
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let dag = self.dag.read().await;
        dag.clone_data(ctx).await
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        let dag = self.dag.read().await;
        dag.full_idmap_clone_data(ctx).await
    }
}
