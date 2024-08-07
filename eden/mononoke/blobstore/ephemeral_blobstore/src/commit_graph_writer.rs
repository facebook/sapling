/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use changesets::ChangesetInsert;
use changesets::Changesets;
use commit_graph::ChangesetParents;
use commit_graph::CommitGraphWriter;
use commit_graph::ParentsFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;
use vec1::Vec1;

use crate::EphemeralChangesets;

pub struct EphemeralCommitGraphWriter {
    ephemeral_changesets: Arc<EphemeralChangesets>,
}

impl EphemeralCommitGraphWriter {
    pub fn new(ephemeral_changesets: Arc<EphemeralChangesets>) -> Self {
        Self {
            ephemeral_changesets,
        }
    }
}

#[async_trait]
impl CommitGraphWriter for EphemeralCommitGraphWriter {
    async fn add(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<bool> {
        let cs_insert = ChangesetInsert {
            cs_id,
            parents: parents.to_vec(),
        };
        self.ephemeral_changesets.add(ctx, cs_insert).await
    }

    async fn add_many(
        &self,
        ctx: &CoreContext,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        let cs_inserts = changesets.mapped(|(cs_id, parents)| ChangesetInsert {
            cs_id,
            parents: parents.to_vec(),
        });

        let cs_count = cs_inserts.len();

        // The number of changesets added is only used for logging, so just return
        // the number of changesets we were given for now.
        self.ephemeral_changesets
            .add_many(ctx, cs_inserts)
            .await
            .map(|()| cs_count)
    }

    async fn add_recursive(
        &self,
        _ctx: &CoreContext,
        _parents_fetcher: Arc<dyn ParentsFetcher>,
        _changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        unimplemented!("add_recursive is not implemented for EphemeralCommitGraphWriter")
    }
}
