/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangesetMut, ChangesetId, DateTime, Timestamp};
use slog::info;

use pushrebase_hook::{
    PushrebaseCommitHook, PushrebaseHook, PushrebaseTransactionHook, RebasedChangesets,
};
use sql::Transaction;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Copy, Clone, Debug)]
pub enum Target {
    Hg(HgChangesetId),
    Bonsai(ChangesetId),
}

impl Target {
    pub fn hg(cs_id: HgChangesetId) -> Self {
        Self::Hg(cs_id)
    }

    pub fn bonsai(cs_id: ChangesetId) -> Self {
        Self::Bonsai(cs_id)
    }
}

#[derive(Clone)]
pub struct UnbundleReplayHook {
    repo: BlobRepo,
    timestamps: Arc<HashMap<ChangesetId, Timestamp>>, // TODO: it'd be nice the hooks could be a lifetime.
    target: Target,
}

impl UnbundleReplayHook {
    pub fn new(
        repo: BlobRepo,
        timestamps: Arc<HashMap<ChangesetId, Timestamp>>,
        target: Target,
    ) -> Box<dyn PushrebaseHook> {
        Box::new(Self {
            repo,
            timestamps,
            target,
        })
    }
}

#[async_trait]
impl PushrebaseHook for UnbundleReplayHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(self.clone()) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

#[async_trait]
impl PushrebaseCommitHook for UnbundleReplayHook {
    fn post_rebase_changeset(
        &mut self,
        bcs_old: ChangesetId,
        bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        let timestamp = self.timestamps.get(&bcs_old).ok_or(format_err!(
            "Attempted to rebase BonsaiChangeset that is not known: {:?}",
            bcs_old
        ))?;

        let tz_offset_secs = bcs_new.author_date.tz_offset_secs();
        let newdate = DateTime::from_timestamp(timestamp.timestamp_seconds(), tz_offset_secs)?;
        bcs_new.author_date = newdate;

        Ok(())
    }

    async fn into_transaction_hook(
        self: Box<Self>,
        ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        let changesets = rebased
            .values()
            .map(|(cs_id, _ts)| *cs_id)
            .collect::<Vec<_>>();

        info!(
            ctx.logger(),
            "Deriving {} hg changesets...",
            changesets.len()
        );

        let mapping = self
            .repo
            .get_hg_bonsai_mapping(ctx.clone(), changesets)
            .await?;

        let ok = mapping
            .into_iter()
            .any(|(hg_cs_id, bonsai_cs_id)| match self.target {
                Target::Hg(t) => t == hg_cs_id,
                Target::Bonsai(t) => t == bonsai_cs_id,
            });

        if !ok {
            return Err(format_err!(
                "Expected target ({:?}) is not found",
                self.target
            ));
        }

        Ok(Box::new(Noop) as Box<dyn PushrebaseTransactionHook>)
    }
}

#[derive(Clone)]
struct Noop;

#[async_trait]
impl PushrebaseTransactionHook for Noop {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        Ok(txn)
    }
}
