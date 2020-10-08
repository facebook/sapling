/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::{
    compat::Future01CompatExt,
    stream::{FuturesOrdered, StreamExt},
    try_join,
};
use tokio::sync::RwLock;

use dag::{self, Id as Vertex};
use stats::prelude::*;

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::{Dag, StartState};
use crate::idmap::IdMap;
use crate::SegmentedChangelog;

define_stats! {
    prefix = "mononoke.segmented_changelog.ondemand";
    build_incremental: timeseries(Sum),
    location_to_changeset_id: timeseries(Sum),
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
        known: ChangesetId,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        {
            let dag = self.dag.read().await;
            if let Some(known_vertex) = dag.idmap.find_vertex(ctx, known).await? {
                return dag
                    .known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
                    .await;
            }
        }
        let known_vertex = {
            let mut dag = self.dag.write().await;
            build_incremental(ctx, &mut dag, &self.changeset_fetcher, known).await?
        };
        let dag = self.dag.read().await;
        dag.known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
            .await
    }
}

async fn build_incremental(
    ctx: &CoreContext,
    dag: &mut Dag,
    changeset_fetcher: &dyn ChangesetFetcher,
    head: ChangesetId,
) -> Result<Vertex> {
    STATS::build_incremental.add_value(1);
    let mut visited = HashSet::new();
    let mut start_state = StartState::new();
    let idmap = &dag.idmap;
    {
        let mut queue = FuturesOrdered::new();
        queue.push(get_parents_and_vertex(ctx, idmap, changeset_fetcher, head));

        while let Some(entry) = queue.next().await {
            let (cs_id, parents, vertex) = entry?;
            start_state.insert_parents(cs_id, parents.clone());
            match vertex {
                Some(v) => {
                    if cs_id == head {
                        return Ok(v);
                    }
                    start_state.insert_vertex_assignment(cs_id, v);
                }
                None => {
                    for parent in parents {
                        if visited.insert(parent) {
                            queue.push(get_parents_and_vertex(
                                ctx,
                                idmap,
                                changeset_fetcher,
                                parent,
                            ));
                        }
                    }
                }
            }
        }
    }

    let low_vertex = idmap
        .get_last_entry(ctx)
        .await?
        .map_or_else(|| dag::Group::MASTER.min_id(), |(vertex, _)| vertex + 1);

    dag.build(ctx, low_vertex, head, start_state).await
}

async fn get_parents_and_vertex(
    ctx: &CoreContext,
    idmap: &dyn IdMap,
    changeset_fetcher: &dyn ChangesetFetcher,
    cs_id: ChangesetId,
) -> Result<(ChangesetId, Vec<ChangesetId>, Option<Vertex>)> {
    let (parents, vertex) = try_join!(
        changeset_fetcher.get_parents(ctx.clone(), cs_id).compat(),
        idmap.find_vertex(ctx, cs_id)
    )?;
    Ok((cs_id, parents, vertex))
}
