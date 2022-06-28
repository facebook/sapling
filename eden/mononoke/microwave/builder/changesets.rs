/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use cloned::cloned;
use context::CoreContext;
use futures::channel::mpsc::Sender;
use futures::sink::SinkExt;
use futures::stream::BoxStream;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use std::sync::Arc;

#[derive(Clone)]
pub struct MicrowaveChangesets {
    repo_id: RepositoryId,
    recorder: Sender<ChangesetEntry>,
    inner: Arc<dyn Changesets>,
}

impl MicrowaveChangesets {
    pub fn new(recorder: Sender<ChangesetEntry>, inner: Arc<dyn Changesets>) -> Self {
        Self {
            repo_id: inner.repo_id(),
            recorder,
            inner,
        }
    }
}

#[async_trait]
impl Changesets for MicrowaveChangesets {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, _ctx: CoreContext, _cs: ChangesetInsert) -> Result<bool, Error> {
        // See rationale in filenodes.rs for why we error out on unexpected calls under
        // MicrowaveFilenodes.
        unimplemented!(
            "MicrowaveChangesets: unexpected add in repo {}",
            self.repo_id
        )
    }

    async fn get(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        cloned!(self.inner, mut self.recorder);

        let entry = inner.get(ctx, cs_id).await?;

        if let Some(ref entry) = entry {
            // NOTE: See MicrowaveFilenodes for context on this.
            assert_eq!(entry.repo_id, self.repo_id);
            recorder.send(entry.clone()).await?;
        }

        Ok(entry)
    }

    async fn get_many(
        &self,
        _ctx: CoreContext,
        _cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        unimplemented!(
            "MicrowaveChangesets: unexpected get_many in repo {}",
            self.repo_id
        )
    }

    async fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        unimplemented!(
            "MicrowaveChangesets: unexpected get_many_by_prefix in repo {}",
            self.repo_id
        )
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }

    async fn enumeration_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
        known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>, Error> {
        self.inner
            .enumeration_bounds(ctx, read_from_master, known_heads)
            .await
    }

    fn list_enumeration_range(
        &self,
        ctx: &CoreContext,
        min_id: u64,
        max_id: u64,
        sort_and_limit: Option<(SortOrder, u64)>,
        read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64), Error>> {
        self.inner
            .list_enumeration_range(ctx, min_id, max_id, sort_and_limit, read_from_master)
    }
}
