/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use changesets::{ChangesetEntry, ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use futures::{
    channel::mpsc::Sender,
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
    sink::SinkExt,
};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{
    ChangesetId, ChangesetIdPrefix, ChangesetIdsResolvedFromPrefix, RepositoryId,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct MicrowaveChangesets {
    repo_id: RepositoryId,
    recorder: Sender<ChangesetEntry>,
    inner: Arc<dyn Changesets>,
}

impl MicrowaveChangesets {
    pub fn new(
        repo_id: RepositoryId,
        recorder: Sender<ChangesetEntry>,
        inner: Arc<dyn Changesets>,
    ) -> Self {
        Self {
            repo_id,
            recorder,
            inner,
        }
    }
}

impl Changesets for MicrowaveChangesets {
    fn add(&self, _ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        // See rationale in filenodes.rs for why we error out on unexpected calls under
        // MicrowaveFilenodes.
        unimplemented!("MicrowaveChangesets: unexpected add in repo {}", cs.repo_id)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        cloned!(self.inner, mut self.recorder);

        // NOTE: See MicrowaveFilenodes for context on this.
        assert_eq!(repo_id, self.repo_id);

        async move {
            let entry = inner.get(ctx, repo_id, cs_id).compat().await?;

            if let Some(ref entry) = entry {
                assert_eq!(entry.repo_id, repo_id); // Same as above
                recorder.send(entry.clone()).await?;
            }

            Ok(entry)
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn get_many(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        _cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        // Same as above
        unimplemented!(
            "MicrowaveChangesets: unexpected get_many in repo {}",
            repo_id
        )
    }

    fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> BoxFuture<ChangesetIdsResolvedFromPrefix, Error> {
        // Same as above
        unimplemented!(
            "MicrowaveChangesets: unexpected get_many_by_prefix in repo {}",
            repo_id
        )
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }
}
