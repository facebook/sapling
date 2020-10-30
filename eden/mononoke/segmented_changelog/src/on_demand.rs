/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures::compat::Future01CompatExt;
use futures::stream::{FuturesOrdered, StreamExt};
use futures::try_join;
use slog::{debug, warn};
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
            if let Some(known_vertex) = dag
                .idmap
                .find_vertex(ctx, known)
                .await
                .context("fetching vertex for csid")?
            {
                if dag.iddag.contains_id(known_vertex)? {
                    return dag
                        .known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
                        .await
                        .context("ondemand first known_location_many_changest_ids");
                }
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

// The goal is to update the Dag. We need a parents function, provided by changeset_fetcher, and a
// place to start, provided by head. The IdMap assigns Vertexes and the IdDag constructs Segments
// in the Vertex space using the parents function. `Dag::build` expects to be given all the data
// that is needs to do assignments and construct Segments in `StartState`. Special care is taken
// for situations where IdMap has more commits processed than the IdDag. Note that parents of
// commits that are unassigned may have been assigned. This means that IdMap assignments are
// expected in `StartState` whenever we are not starting from scratch.
pub(crate) async fn build_incremental(
    ctx: &CoreContext,
    dag: &mut Dag,
    changeset_fetcher: &dyn ChangesetFetcher,
    head: ChangesetId,
) -> Result<Vertex> {
    STATS::build_incremental.add_value(1);
    let mut visited = HashSet::new();
    let mut start_state = StartState::new();
    let idmap = &dag.idmap;

    let id_dag_next_id = dag
        .iddag
        .next_free_id(0, dag::Group::MASTER)
        .context("fetching next free id")?;
    let id_map_next_id = idmap
        .get_last_entry(ctx)
        .await?
        .map_or_else(|| dag::Group::MASTER.min_id(), |(vertex, _)| vertex + 1);
    if id_dag_next_id > id_map_next_id {
        bail!("id_dag_next_id > id_map_next_id; unexpected state, re-seed the repository");
    }
    if id_dag_next_id < id_map_next_id {
        warn!(
            ctx.logger(),
            "id_dag_next_id < id_map_next_id; this suggests that constructing and saving the iddag \
            is failing or that the idmap generation is racing"
        );
    }

    {
        let mut queue = FuturesOrdered::new();
        queue.push(get_parents_and_vertex(ctx, idmap, changeset_fetcher, head));

        while let Some(entry) = queue.next().await {
            let (cs_id, parents, vertex) = entry?;
            start_state.insert_parents(cs_id, parents.clone());
            if let Some(v) = vertex {
                start_state.insert_vertex_assignment(cs_id, v);
            }
            let vertex_missing_from_iddag = match vertex {
                Some(v) => !dag.iddag.contains_id(v)?,
                None => true,
            };
            if vertex_missing_from_iddag {
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

    if id_dag_next_id == id_map_next_id {
        if let Some(head_vertex) = start_state.assignments.find_vertex(head) {
            debug!(
                ctx.logger(),
                "idmap and iddags already contain head {}, skipping incremental build", head
            );
            return Ok(head_vertex);
        }
    }

    dag.build(ctx, id_map_next_id, head, start_state)
        .await
        .context("incrementally updating dag")
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
