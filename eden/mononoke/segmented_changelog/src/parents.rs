/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;

use crate::dag::errors::BackendError;
use crate::dag::namedag::MemNameDag;
use crate::dag::ops::Parents;
use crate::dag::Result;
use crate::dag::VertexName;
use crate::idmap::cs_id_from_vertex_name;
use crate::idmap::vertex_name_from_cs_id;

pub struct FetchParents {
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
}

impl FetchParents {
    pub fn new(ctx: CoreContext, changeset_fetcher: ArcChangesetFetcher) -> Self {
        Self {
            ctx,
            changeset_fetcher,
        }
    }
}

#[async_trait::async_trait]
impl Parents for FetchParents {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let cs_id = cs_id_from_vertex_name(&name);
        let parents = self
            .changeset_fetcher
            .get_parents(self.ctx.clone(), cs_id)
            .await
            .map_err(BackendError::from)?;

        Ok(parents.iter().map(vertex_name_from_cs_id).collect())
    }

    async fn hint_subdag_for_insertion(&self, _heads: &[VertexName]) -> Result<MemNameDag> {
        // No dirty scope here, so always return empty
        Ok(MemNameDag::new())
    }
}
