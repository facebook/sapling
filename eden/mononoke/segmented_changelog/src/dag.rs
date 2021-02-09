/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, sync::Arc};

use anyhow::{format_err, Context, Result};
use async_trait::async_trait;
use futures::stream::{self, StreamExt, TryStreamExt};
use maplit::hashset;
use slog::{debug, trace};

use cloned::cloned;
use dag::{self, CloneData, Group, Id as Vertex, InProcessIdDag, Location};
use stats::prelude::*;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::{IdMap, MemIdMap};
use crate::{SegmentedChangelog, StreamCloneData};

const IDMAP_CHANGESET_FETCH_BATCH: usize = 500;

define_stats! {
    prefix = "mononoke.segmented_changelog.dag";
    build: timeseries(Sum),
    location_to_changeset_id: timeseries(Sum),
}

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    pub(crate) iddag: InProcessIdDag,
    pub(crate) idmap: Arc<dyn IdMap>,
}

#[async_trait]
impl SegmentedChangelog for Dag {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let location = location
            .and_then_descendant(|hgid| self.idmap.get_vertex(ctx, hgid))
            .await?;
        self.known_location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let group = Group::MASTER;
        let head_id = self.clone_data_head_id()?;
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let universal_ids = self
            .iddag
            .universal_ids()
            .context("error computing universal ids")?
            .into_iter()
            .collect();
        let idmap = self
            .idmap
            .find_many_changeset_ids(&ctx, universal_ids)
            .await
            .context("error retrieving mappings for dag universal ids")?;
        let clone_data = CloneData {
            head_id,
            flat_segments,
            idmap,
        };
        Ok(clone_data)
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        const CHUNK_SIZE: usize = 1000;
        const BUFFERED_BATCHES: usize = 5;
        let group = Group::MASTER;
        let head_id = self.clone_data_head_id()?;
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let idmap_stream = stream::iter((group.min_id().0..=head_id.0).into_iter().map(Vertex))
            .chunks(CHUNK_SIZE)
            .map({
                cloned!(ctx, self.idmap);
                move |chunk| {
                    cloned!(ctx, idmap);
                    async move { idmap.find_many_changeset_ids(&ctx, chunk).await }
                }
            })
            .buffered(BUFFERED_BATCHES)
            .map_ok(|map_chunk| stream::iter(map_chunk.into_iter().map(Ok)))
            .try_flatten()
            .boxed();
        let stream_clone_data = StreamCloneData {
            head_id,
            flat_segments,
            idmap_stream,
        };
        Ok(stream_clone_data)
    }
}

impl Dag {
    pub fn new(iddag: InProcessIdDag, idmap: Arc<dyn IdMap>) -> Self {
        Self { iddag, idmap }
    }

    pub(crate) async fn known_location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<Vertex>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let mut dist_ancestor_vertex = self
            .iddag
            .first_ancestor_nth(location.descendant, location.distance)
            .with_context(|| format!("failed to compute location origin for {:?}", location))?;
        let mut vertexes = vec![dist_ancestor_vertex];
        for _ in 1..count {
            let parents = self
                .iddag
                .parent_ids(dist_ancestor_vertex)
                .with_context(|| format!("looking up parents ids for {}", dist_ancestor_vertex))?;
            if parents.len() != 1 {
                return Err(format_err!(
                    "invalid request: changeset with vertex {} does not have {} single parent ancestors",
                    location.descendant,
                    location.distance + count - 1
                ));
            }
            dist_ancestor_vertex = parents[0];
            vertexes.push(dist_ancestor_vertex);
        }
        let changeset_futures = vertexes
            .into_iter()
            .map(|vertex| self.idmap.get_changeset_id(ctx, vertex));
        stream::iter(changeset_futures)
            .buffered(IDMAP_CHANGESET_FETCH_BATCH)
            .try_collect()
            .await
    }

    pub(crate) async fn build(
        &mut self,
        ctx: &CoreContext,
        low_vertex: Vertex,
        head: ChangesetId,
        start_state: StartState,
    ) -> Result<Vertex> {
        enum Todo {
            Visit(ChangesetId),
            Assign(ChangesetId),
        }
        let mut todo_stack = vec![Todo::Visit(head)];
        let mut mem_idmap = MemIdMap::new();
        let mut seen = hashset![head];

        while let Some(todo) = todo_stack.pop() {
            match todo {
                Todo::Visit(cs_id) => {
                    let parents = match start_state.get_parents_if_not_assigned(cs_id) {
                        None => continue,
                        Some(v) => v,
                    };
                    todo_stack.push(Todo::Assign(cs_id));
                    for parent in parents.iter().rev() {
                        // Note: iterating parents in reverse is a small optimization because
                        // in our setup p1 is master.
                        if seen.insert(*parent) {
                            todo_stack.push(Todo::Visit(*parent));
                        }
                    }
                }
                Todo::Assign(cs_id) => {
                    let vertex = low_vertex + mem_idmap.len() as u64;
                    mem_idmap.insert(vertex, cs_id);
                    trace!(
                        ctx.logger(),
                        "assigning vertex id '{}' to changeset id '{}'",
                        vertex,
                        cs_id
                    );
                }
            }
        }

        let head_vertex = mem_idmap
            .find_vertex(head)
            .or_else(|| start_state.assignments.find_vertex(head))
            .ok_or_else(|| format_err!("error building IdMap; failed to assign head {}", head))?;

        debug!(
            ctx.logger(),
            "inserting {} entries into IdMap",
            mem_idmap.len()
        );
        self.idmap
            .insert_many(ctx, mem_idmap.iter().collect::<Vec<_>>())
            .await?;
        debug!(ctx.logger(), "successully inserted entries to IdMap");

        let get_vertex_parents = |vertex: Vertex| -> dag::Result<Vec<Vertex>> {
            let cs_id = match mem_idmap.find_changeset_id(vertex) {
                None => start_state
                    .assignments
                    .get_changeset_id(vertex)
                    .map_err(|e| dag::errors::BackendError::Other(e))?,
                Some(v) => v,
            };
            let parents = start_state.parents.get(&cs_id).ok_or_else(|| {
                let err = format_err!(
                    "error building IdMap; unexpected request for parents for {}",
                    cs_id
                );
                dag::errors::BackendError::Other(err)
            })?;
            let mut response = Vec::with_capacity(parents.len());
            for parent in parents {
                let vertex = match mem_idmap.find_vertex(*parent) {
                    None => start_state
                        .assignments
                        .get_vertex(*parent)
                        .map_err(|e| dag::errors::BackendError::Other(e))?,
                    Some(v) => v,
                };
                response.push(vertex);
            }
            Ok(response)
        };

        // TODO(sfilip, T67731559): Prefetch parents for IdDag from last processed Vertex
        debug!(ctx.logger(), "building iddag");
        self.iddag
            .build_segments_volatile(head_vertex, &get_vertex_parents)
            .context("building iddag")?;
        debug!(
            ctx.logger(),
            "successfully finished building building iddag"
        );

        Ok(head_vertex)
    }

    fn clone_data_head_id(&self) -> Result<Vertex> {
        let group = Group::MASTER;
        let level = 0;
        let next_id = self
            .iddag
            .next_free_id(level, group)
            .context("error computing next free id for dag")?;
        if next_id > group.min_id() {
            Ok(next_id - 1)
        } else {
            Err(format_err!("error generating clone data for empty iddag"))
        }
    }
}

// TODO(sfilip): use a dedicated parents structure which specializes the case where
// we have 0, 1 and 2 parents, 3+ is a 4th variant backed by Vec.
// Note: the segment construction algorithm will want to query the vertexes of the parents
// that were already assigned.
#[derive(Debug)]
pub(crate) struct StartState {
    pub(crate) parents: HashMap<ChangesetId, Vec<ChangesetId>>,
    pub(crate) assignments: MemIdMap,
}

impl StartState {
    pub fn new() -> Self {
        Self {
            parents: HashMap::new(),
            assignments: MemIdMap::new(),
        }
    }

    pub fn insert_parents(
        &mut self,
        cs_id: ChangesetId,
        parents: Vec<ChangesetId>,
    ) -> Option<Vec<ChangesetId>> {
        self.parents.insert(cs_id, parents)
    }

    pub fn insert_vertex_assignment(&mut self, cs_id: ChangesetId, vertex: Vertex) {
        self.assignments.insert(vertex, cs_id)
    }

    // The purpose of the None return value is to signal that the changeset has already been assigned
    // This is useful in the incremental build step when we traverse back through parents. Normally
    // we would check the idmap at each iteration step but we have the information prefetched when
    // getting parents data.
    pub fn get_parents_if_not_assigned(&self, cs_id: ChangesetId) -> Option<Vec<ChangesetId>> {
        if self.assignments.find_vertex(cs_id).is_some() {
            return None;
        }
        self.parents.get(&cs_id).cloned()
    }
}
