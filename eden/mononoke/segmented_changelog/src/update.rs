/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::future::Future;

use anyhow::{bail, format_err, Context, Result};
use futures::stream::{FuturesOrdered, StreamExt};
use futures::try_join;
use slog::{debug, trace, warn};

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::idmap::IdMapAssignHead;
use crate::dag::IdSet;
use crate::iddag::rebuild::rebuild_iddag;
use crate::idmap::{vertex_name_from_cs_id, IdMap, IdMapWrapper, MemIdMap};
use crate::parents::FetchParents;
use crate::{dag, DagId, Group, InProcessIdDag};

//TODO(simonfar): For some reason, building the IdDag from prepared flat segments
//doesn't work reliably. For now, we always rebuild the IdDag from commit history instead.
const REBUILD_IDDAG: bool = true;

pub fn update_sc<'a>(
    ctx: &'a CoreContext,
    parent_fetcher: &'a FetchParents,
    iddag: &'a mut InProcessIdDag,
    idmap: &'a dyn IdMap,
    head: ChangesetId,
) -> impl Future<Output = Result<usize>> + 'a {
    async move {
        let mut covered_ids = iddag.all()?;
        let flat_segments = IdMapWrapper::run(ctx.clone(), idmap, move |mut idmap| async move {
            idmap
                .assign_head(
                    vertex_name_from_cs_id(&head),
                    parent_fetcher,
                    Group::MASTER,
                    &mut covered_ids,
                    &IdSet::empty(),
                )
                .await
                .map_err(anyhow::Error::from)
        })
        .await?;
        if REBUILD_IDDAG || flat_segments.segment_count() == 0 {
            return rebuild_iddag(ctx, parent_fetcher, idmap, iddag, head, REBUILD_IDDAG).await;
        }

        iddag.build_segments_from_prepared_flat_segments(&flat_segments)?;
        Ok(flat_segments.segment_count())
    }
}

// TODO(sfilip): use a dedicated parents structure which specializes the case where
// we have 0, 1 and 2 parents, 3+ is a 4th variant backed by Vec.
// Note: the segment construction algorithm will want to query the dag_ids of the parents
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

    pub fn insert_dag_id_assignment(&mut self, cs_id: ChangesetId, dag_id: DagId) {
        self.assignments.insert(dag_id, cs_id)
    }

    // The purpose of the None return value is to signal that the changeset has already been assigned
    // This is useful in the incremental build step when we traverse back through parents. Normally
    // we would check the idmap at each iteration step but we have the information prefetched when
    // getting parents data.
    pub fn get_parents_if_not_assigned(&self, cs_id: ChangesetId) -> Option<Vec<ChangesetId>> {
        if self.assignments.find_dag_id(cs_id).is_some() {
            return None;
        }
        self.parents.get(&cs_id).cloned()
    }
}

pub fn assign_ids(
    ctx: &CoreContext,
    start_state: &StartState,
    head: ChangesetId,
    low_dag_id: DagId,
) -> Result<(MemIdMap, DagId)> {
    let mut mem_idmap = MemIdMap::new();

    let head_dag_id = assign_ids_with_id_map(ctx, start_state, head, low_dag_id, &mut mem_idmap)?;

    Ok((mem_idmap, head_dag_id))
}

pub fn assign_ids_with_id_map(
    ctx: &CoreContext,
    start_state: &StartState,
    head: ChangesetId,
    low_dag_id: DagId,
    mem_idmap: &mut MemIdMap,
) -> Result<DagId> {
    enum Todo {
        Visit(ChangesetId),
        Assign(ChangesetId),
    }
    let mut todo_stack = vec![Todo::Visit(head)];

    while let Some(todo) = todo_stack.pop() {
        match todo {
            Todo::Visit(cs_id) => {
                if mem_idmap.find_dag_id(cs_id).is_some() {
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
                if mem_idmap.find_dag_id(cs_id).is_some() {
                    continue;
                }
                let dag_id = low_dag_id + mem_idmap.len() as u64;
                mem_idmap.insert(dag_id, cs_id);
                trace!(
                    ctx.logger(),
                    "assigning dag_id id '{}' to changeset id '{}'",
                    dag_id,
                    cs_id
                );
            }
        }
    }

    let head_dag_id = mem_idmap
        .find_dag_id(head)
        .or_else(|| start_state.assignments.find_dag_id(head))
        .ok_or_else(|| format_err!("error assigning ids; failed to assign head {}", head))?;

    let cs_to_v = |cs| {
        mem_idmap
            .find_dag_id(cs)
            .or_else(|| start_state.assignments.find_dag_id(cs))
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
                        "error assigning ids; parent >= dag_id: {} >= {} ({} >= {})",
                        pv,
                        v,
                        p,
                        cs
                    ));
                }
            }
        }
    }

    Ok(head_dag_id)
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
    head_dag_id: DagId,
) -> Result<()> {
    let get_dag_id_parents = |dag_id: DagId| -> dag::Result<Vec<DagId>> {
        let cs_id = match mem_idmap.find_changeset_id(dag_id) {
            None => start_state
                .assignments
                .get_changeset_id(dag_id)
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
            let pv = match mem_idmap.find_dag_id(*parent) {
                None => start_state
                    .assignments
                    .get_dag_id(*parent)
                    .map_err(dag::errors::BackendError::Other)?,
                Some(v) => v,
            };
            response.push(pv);
        }
        Ok(response)
    };

    // TODO(sfilip, T67731559): Prefetch parents for IdDag from last processed DagId
    debug!(ctx.logger(), "building iddag");
    iddag
        .build_segments(head_dag_id, &get_dag_id_parents)
        .context("building iddag")?;
    debug!(ctx.logger(), "successfully finished updating iddag");
    Ok(())
}

pub async fn prepare_incremental_iddag_update<'a>(
    ctx: &'a CoreContext,
    iddag: &'a InProcessIdDag,
    idmap: &'a dyn IdMap,
    changeset_fetcher: &'a dyn ChangesetFetcher,
    head: ChangesetId,
) -> Result<(DagId, Option<(StartState, MemIdMap)>)> {
    let mut visited = HashSet::new();
    let mut start_state = StartState::new();

    let id_dag_covered_id_set = iddag.master_group().context("iddag::master_group()")?;
    let id_dag_next_id = id_dag_covered_id_set
        .max()
        .map(|dag_id| dag_id + 1)
        .unwrap_or_else(|| dag::Group::MASTER.min_id());
    let id_map_next_id = idmap
        .get_last_entry(ctx)
        .await?
        .map_or_else(|| dag::Group::MASTER.min_id(), |(dag_id, _)| dag_id + 1);
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
        queue.push(get_parents_and_dag_id(ctx, idmap, changeset_fetcher, head));

        while let Some(entry) = queue.next().await {
            let (cs_id, parents, dag_id) = entry?;
            start_state.insert_parents(cs_id, parents.clone());
            if let Some(v) = dag_id {
                if v < id_map_next_id {
                    start_state.insert_dag_id_assignment(cs_id, v);
                } else {
                    return Err(format_err!(
                        "racing data while updating segmented changelog, \
                        next_id is {} but found {} assigned",
                        id_map_next_id,
                        v
                    ));
                }
            }
            let dag_id_missing_from_iddag = match dag_id {
                Some(v) => !iddag.contains_id(v)?,
                None => true,
            };
            if dag_id_missing_from_iddag {
                for parent in parents {
                    if visited.insert(parent) {
                        queue.push(get_parents_and_dag_id(
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
        if let Some(head_dag_id) = start_state.assignments.find_dag_id(head) {
            debug!(
                ctx.logger(),
                "idmap and iddags already contain head {}, skipping incremental build", head
            );
            return Ok((head_dag_id, None));
        }
    }

    let (mem_idmap, head_dag_id) = assign_ids(ctx, &start_state, head, id_map_next_id)?;

    update_idmap(ctx, idmap, &mem_idmap).await?;

    Ok((head_dag_id, Some((start_state, mem_idmap))))
}

async fn get_parents_and_dag_id(
    ctx: &CoreContext,
    idmap: &dyn IdMap,
    changeset_fetcher: &dyn ChangesetFetcher,
    cs_id: ChangesetId,
) -> Result<(ChangesetId, Vec<ChangesetId>, Option<DagId>)> {
    let (parents, dag_id) = try_join!(
        changeset_fetcher.get_parents(ctx.clone(), cs_id),
        idmap.find_dag_id(ctx, cs_id)
    )?;
    Ok((cs_id, parents, dag_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID};

    #[fbinit::test]
    async fn test_assign_ids(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let mut start_state = StartState::new();
        start_state.insert_parents(ONES_CSID, vec![]);
        start_state.insert_parents(TWOS_CSID, vec![ONES_CSID]);
        start_state.insert_parents(THREES_CSID, vec![ONES_CSID, TWOS_CSID]);
        start_state.insert_parents(FOURS_CSID, vec![TWOS_CSID, THREES_CSID]);

        let (mem_idmap, head_dag_id) = assign_ids(&ctx, &start_state, FOURS_CSID, DagId(1))?;
        assert_eq!(head_dag_id, DagId(4));
        assert_eq!(mem_idmap.get_dag_id(ONES_CSID)?, DagId(1));
        assert_eq!(mem_idmap.get_dag_id(TWOS_CSID)?, DagId(2));
        assert_eq!(mem_idmap.get_dag_id(THREES_CSID)?, DagId(3));
        assert_eq!(mem_idmap.get_dag_id(FOURS_CSID)?, DagId(4));

        // Vary parent order because that has an impact on the order nodes are assigned
        let mut start_state = StartState::new();
        start_state.insert_parents(ONES_CSID, vec![]);
        start_state.insert_parents(TWOS_CSID, vec![ONES_CSID]);
        start_state.insert_parents(THREES_CSID, vec![TWOS_CSID, ONES_CSID]);
        start_state.insert_parents(FOURS_CSID, vec![THREES_CSID, TWOS_CSID]);

        let (mem_idmap, head_dag_id) = assign_ids(&ctx, &start_state, FOURS_CSID, DagId(1))?;
        assert_eq!(head_dag_id, DagId(4));
        assert_eq!(mem_idmap.get_dag_id(ONES_CSID)?, DagId(1));
        assert_eq!(mem_idmap.get_dag_id(TWOS_CSID)?, DagId(2));
        assert_eq!(mem_idmap.get_dag_id(THREES_CSID)?, DagId(3));
        assert_eq!(mem_idmap.get_dag_id(FOURS_CSID)?, DagId(4));

        Ok(())
    }
}
