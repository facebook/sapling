/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use futures::compat::Future01CompatExt;

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;

pub struct Parents<'a> {
    ctx: &'a CoreContext,
    blob_repo: &'a BlobRepo,
}

impl<'a> Parents<'a> {
    pub fn new(ctx: &'a CoreContext, blob_repo: &'a BlobRepo) -> Parents<'a> {
        Parents { ctx, blob_repo }
    }
    pub async fn get(&self, changeset_id: ChangesetId) -> Result<Vec<ChangesetId>> {
        let parents = self
            .blob_repo
            .get_changeset_parents_by_bonsai(self.ctx.clone(), changeset_id)
            .compat()
            .await?;
        Ok(parents)
    }
}

// TODO(sfilip):
// generate_graph
// struct Dag(idmap, segments)
