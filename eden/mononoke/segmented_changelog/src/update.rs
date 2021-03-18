/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};

use anyhow::{bail, format_err, Context, Result};
use futures::stream::{FuturesOrdered, StreamExt};
use futures::try_join;
use slog::{debug, trace, warn};

use dag::{Id as Vertex, InProcessIdDag};

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::{IdMap, MemIdMap};
use crate::owned::OwnedSegmentedChangelog;

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
) -> Result<(MemIdMap, Vertex)> {
    enum Todo {
        Visit(ChangesetId),
        Assign(ChangesetId),
    }
    let mut todo_stack = vec![Todo::Visit(head)];
    let mut mem_idmap = MemIdMap::new();

    while let Some(todo) = todo_stack.pop() {
        match todo {
            Todo::Visit(cs_id) => {
                if mem_idmap.find_vertex(cs_id).is_some() {
                    continue;
                }
                let parents = match start_state.get_parents_if_not_assigned(cs_id) {
                    None => continue,
                    Some(v) => v,
                };
                todo_stack.push(Todo::Assign(cs_id));
                for parent in parents.iter().rev() {
                    // Note: iterating parents in reverse is a small optimization because
                    // in our setup p1 is master.
                    todo_stack.push(Todo::Visit(*parent));
                }
            }
            Todo::Assign(cs_id) => {
                if mem_idmap.find_vertex(cs_id).is_some() {
                    continue;
                }
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
        .ok_or_else(|| format_err!("error assigning ids; failed to assign head {}", head))?;

    let cs_to_v = |cs| {
        mem_idmap
            .find_vertex(cs)
            .or_else(|| start_state.assignments.find_vertex(cs))
            .ok_or_else(|| {
                format_err!(
                    "error assingning ids; failed to find assignment for changeset {}",
                    cs
                )
            })
    };
    for (v, cs) in mem_idmap.iter() {
        if let Some(parents) = start_state.parents.get(&cs) {
            for p in parents {
                let pv = cs_to_v(*p)?;
                if pv >= v {
                    return Err(format_err!(
                        "error assigning ids; parent >= vertex: {} >= {} ({} >= {})",
                        pv,
                        v,
                        p,
                        cs
                    ));
                }
            }
        }
    }

    Ok((mem_idmap, head_vertex))
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
            let pv = match mem_idmap.find_vertex(*parent) {
                None => start_state
                    .assignments
                    .get_vertex(*parent)
                    .map_err(dag::errors::BackendError::Other)?,
                Some(v) => v,
            };
            response.push(pv);
        }
        Ok(response)
    };

    // TODO(sfilip, T67731559): Prefetch parents for IdDag from last processed Vertex
    debug!(ctx.logger(), "building iddag");
    iddag
        .build_segments_volatile(head_vertex, &get_vertex_parents)
        .context("building iddag")?;
    debug!(ctx.logger(), "successfully finished updating iddag");
    Ok(())
}

// The goal is to update the Dag. We need a parents function, provided by changeset_fetcher, and a
// place to start, provided by head. The IdMap assigns Vertexes and the IdDag constructs Segments
// in the Vertex space using the parents function. `Dag::build` expects to be given all the data
// that is needs to do assignments and construct Segments in `StartState`. Special care is taken
// for situations where IdMap has more commits processed than the IdDag. Note that parents of
// commits that are unassigned may have been assigned. This means that IdMap assignments are
// expected in `StartState` whenever we are not starting from scratch.
pub async fn build_incremental(
    ctx: &CoreContext,
    sc: &mut OwnedSegmentedChangelog,
    changeset_fetcher: &dyn ChangesetFetcher,
    head: ChangesetId,
) -> Result<Vertex> {
    let (head_vertex, maybe_iddag_update) =
        prepare_incremental_iddag_update(ctx, &sc.iddag, &sc.idmap, changeset_fetcher, head)
            .await
            .context("error preparing an incremental update for iddag")?;

    if let Some((start_state, mem_idmap)) = maybe_iddag_update {
        update_iddag(ctx, &mut sc.iddag, &start_state, &mem_idmap, head_vertex)?;
    }

    Ok(head_vertex)
}

pub async fn prepare_incremental_iddag_update<'a>(
    ctx: &'a CoreContext,
    iddag: &'a InProcessIdDag,
    idmap: &'a dyn IdMap,
    changeset_fetcher: &'a dyn ChangesetFetcher,
    head: ChangesetId,
) -> Result<(Vertex, Option<(StartState, MemIdMap)>)> {
    let mut visited = HashSet::new();
    let mut start_state = StartState::new();

    let id_dag_next_id = iddag
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
                if v < id_map_next_id {
                    start_state.insert_vertex_assignment(cs_id, v);
                } else {
                    return Err(format_err!(
                        "racing data while updating segmented changelog, \
                        next_id is {} but found {} assigned",
                        id_map_next_id,
                        v
                    ));
                }
            }
            let vertex_missing_from_iddag = match vertex {
                Some(v) => !iddag.contains_id(v)?,
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
            return Ok((head_vertex, None));
        }
    }

    let (mem_idmap, head_vertex) = assign_ids(ctx, &start_state, head, id_map_next_id)?;

    update_idmap(ctx, idmap, &mem_idmap).await?;

    Ok((head_vertex, Some((start_state, mem_idmap))))
}

async fn get_parents_and_vertex(
    ctx: &CoreContext,
    idmap: &dyn IdMap,
    changeset_fetcher: &dyn ChangesetFetcher,
    cs_id: ChangesetId,
) -> Result<(ChangesetId, Vec<ChangesetId>, Option<Vertex>)> {
    let (parents, vertex) = try_join!(
        changeset_fetcher.get_parents(ctx.clone(), cs_id),
        idmap.find_vertex(ctx, cs_id)
    )?;
    Ok((cs_id, parents, vertex))
}
