/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, VecDeque};

use anyhow::{anyhow, Result};
use context::CoreContext;
use mononoke_types::ChangesetId;
use slog::warn;

use crate::dag::errors::NotFoundError;
use crate::dag::{Id, InProcessIdDag};
use crate::{parents::FetchParents, IdMap};

pub async fn rebuild_iddag(
    ctx: &CoreContext,
    parent_fetcher: &FetchParents,
    idmap: &dyn IdMap,
    iddag: &mut InProcessIdDag,
    head: ChangesetId,
    missing_head_expected: bool,
) -> Result<usize> {
    let head_id = idmap
        .find_dag_id(ctx, head)
        .await?
        .ok_or_else(|| anyhow!("Just added head {} is not in IdMap", head))?;
    if !iddag.contains_id(head_id)? {
        if !missing_head_expected {
            warn!(
                ctx.logger(),
                "IdDag does not contain {} which is in the IdMap. Building IdDag", head
            );
        }
        let parents =
            load_idmap_parents_not_in_iddag(ctx, parent_fetcher, idmap, iddag, head_id).await?;
        iddag
            .build_segments(head_id, &|id| {
                parents
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| id.not_found_error())
            })
            .map_err(anyhow::Error::from)
    } else {
        Ok(0)
    }
}

async fn load_idmap_parents_not_in_iddag(
    ctx: &CoreContext,
    parent_fetcher: &FetchParents,
    idmap: &dyn IdMap,
    iddag: &InProcessIdDag,
    head_id: Id,
) -> Result<HashMap<Id, Vec<Id>>> {
    let changeset_fetcher = parent_fetcher.get_changeset_fetcher();

    // We're going to load all the Ids in the IdMap that aren't in the iddag,
    // and track their parents.
    let mut res = HashMap::new();
    let mut ids_to_find = VecDeque::new();

    ids_to_find.push_back(head_id);
    while let Some(head) = ids_to_find.pop_front() {
        let head_cs_id = idmap
            .find_changeset_id(ctx, head)
            .await?
            .ok_or_else(|| head.not_found_error())?;
        let parents = changeset_fetcher
            .get_parents(ctx.clone(), head_cs_id)
            .await?;
        let parent_ids = idmap.find_many_dag_ids(ctx, parents.clone()).await?;
        let parents: Vec<Id> = parents
            .into_iter()
            .map(|id| parent_ids.get(&id).copied().ok_or_else(|| anyhow!("Changeset {} not found in segmented changelog, yet should be present - reseed!", id)))
            .collect::<Result<_>>()?;
        for parent in &parents {
            let known_id = iddag.contains_id(*parent)? || res.contains_key(parent);
            if !known_id {
                ids_to_find.push_back(*parent);
            }
        }
        res.insert(head, parents);
    }
    Ok(res)
}
