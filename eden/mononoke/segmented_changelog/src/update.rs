/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::{NameDagBuilder, VertexName, VertexOptions};
use crate::idmap::{vertex_name_from_cs_id, IdMap, IdMapWrapper};
use crate::{Group, InProcessIdDag};

pub type ServerNameDag = crate::dag::namedag::AbstractNameDag<InProcessIdDag, IdMapWrapper, (), ()>;

/// Convert a server IdDag and IdMap to a NameDag
/// Note: you will need to call NameDag::as_idmap().flush_writes
/// to write out updates to the IdMap
pub fn server_namedag(
    ctx: CoreContext,
    iddag: InProcessIdDag,
    idmap: Arc<dyn IdMap>,
) -> Result<ServerNameDag> {
    let idmap = IdMapWrapper::new(ctx, idmap);
    NameDagBuilder::new_with_idmap_dag(idmap, iddag)
        .build()
        .map_err(anyhow::Error::from)
}

pub fn head_with_options(head: ChangesetId) -> (VertexName, VertexOptions) {
    let mut options = VertexOptions::default();
    options.highest_group = Group::MASTER;
    (vertex_name_from_cs_id(&head), options)
}
