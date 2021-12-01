/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;

use anyhow::Result;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::idmap::IdMapAssignHead;
use crate::dag::IdSet;
use crate::iddag::rebuild::rebuild_iddag;
use crate::idmap::{vertex_name_from_cs_id, IdMap, IdMapWrapper};
use crate::parents::FetchParents;
use crate::{Group, InProcessIdDag};

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
