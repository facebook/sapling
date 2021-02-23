/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{format_err, Context, Result};
use maplit::hashset;
use slog::{debug, trace};

use dag::{Id as Vertex, InProcessIdDag};
use stats::prelude::*;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::{IdMap, MemIdMap};

define_stats! {
    build: timeseries(Sum),
}

pub async fn build<'a>(
    ctx: &'a CoreContext,
    iddag: &'a mut InProcessIdDag,
    idmap: &'a dyn IdMap,
    start_state: &'a StartState,
    head: ChangesetId,
    low_vertex: Vertex,
) -> Result<Vertex> {
    let mem_idmap = assign_ids(ctx, &start_state, head, low_vertex);

    let head_vertex = mem_idmap
        .find_vertex(head)
        .or_else(|| start_state.assignments.find_vertex(head))
        .ok_or_else(|| format_err!("error building IdMap; failed to assign head {}", head))?;

    update_idmap(ctx, idmap, &mem_idmap).await?;

    update_iddag(ctx, iddag, start_state, &mem_idmap, head_vertex)?;

    Ok(head_vertex)
}

// TODO(sfilip): use a dedicated parents structure which specializes the case where
// we have 0, 1 and 2 parents, 3+ is a 4th variant backed by Vec.
// Note: the segment construction algorithm will want to query the vertexes of the parents
// that were already assigned.
#[derive(Debug)]
pub struct StartState {
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

pub fn assign_ids(
    ctx: &CoreContext,
    start_state: &StartState,
    head: ChangesetId,
    low_vertex: Vertex,
) -> MemIdMap {
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
    mem_idmap
}

pub async fn update_idmap<'a>(
    ctx: &'a CoreContext,
    idmap: &'a dyn IdMap,
    mem_idmap: &'a MemIdMap,
) -> Result<()> {
    debug!(
        ctx.logger(),
        "inserting {} entries into IdMap",
        mem_idmap.len()
    );
    idmap
        .insert_many(ctx, mem_idmap.iter().collect::<Vec<_>>())
        .await?;
    debug!(ctx.logger(), "successully inserted entries to IdMap");
    Ok(())
}

pub fn update_iddag(
    ctx: &CoreContext,
    iddag: &mut InProcessIdDag,
    start_state: &StartState,
    mem_idmap: &MemIdMap,
    head_vertex: Vertex,
) -> Result<()> {
    let get_vertex_parents = |vertex: Vertex| -> dag::Result<Vec<Vertex>> {
        let cs_id = match mem_idmap.find_changeset_id(vertex) {
            None => start_state
                .assignments
                .get_changeset_id(vertex)
                .map_err(dag::errors::BackendError::Other)?,
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
                    .map_err(dag::errors::BackendError::Other)?,
                Some(v) => v,
            };
            response.push(vertex);
        }
        Ok(response)
    };

    // TODO(sfilip, T67731559): Prefetch parents for IdDag from last processed Vertex
    debug!(ctx.logger(), "building iddag");
    iddag
        .build_segments_volatile(head_vertex, &get_vertex_parents)
        .context("building iddag")?;
    debug!(
        ctx.logger(),
        "successfully finished building building iddag"
    );
    Ok(())
}
