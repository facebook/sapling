/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;

use crate::dag::errors::BackendError;
use crate::dag::namedag::MemNameDag;
use crate::dag::ops::Parents;
use crate::dag::{Result, VertexName};
use crate::idmap::{cs_id_from_vertex_name, vertex_name_from_cs_id};

pub struct FetchParents {
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
}

impl FetchParents {
    pub fn new(ctx: CoreContext, changeset_fetcher: Arc<dyn ChangesetFetcher>) -> Self {
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
