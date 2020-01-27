/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use futures_preview::compat::Future01CompatExt;
use mononoke_types::ChangesetId;
use pushrebase::{
    PushrebaseCommitHook, PushrebaseHook, PushrebaseTransactionHook, RebasedChangesets,
};
use sql::Transaction;

use crate::{create_synced_commit_mapping_entry, CommitSyncRepos};
use synced_commit_mapping::{add_many_in_txn, SyncedCommitMappingEntry};

#[derive(Clone)]
pub struct CrossRepoSyncPushrebaseHook {
    cs_id: ChangesetId,
    repos: CommitSyncRepos,
}

impl CrossRepoSyncPushrebaseHook {
    pub fn new(cs_id: ChangesetId, repos: CommitSyncRepos) -> Box<dyn PushrebaseHook> {
        Box::new(Self { cs_id, repos })
    }
}

#[async_trait]
impl PushrebaseHook for CrossRepoSyncPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(self.clone()) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

impl PushrebaseCommitHook for CrossRepoSyncPushrebaseHook {
    fn into_transaction_hook(
        self: Box<Self>,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        if rebased.len() > 1 {
            return Err(format_err!("expected exactly one commit to be rebased").into());
        }

        match rebased.into_iter().next() {
            Some((_, (new_cs_id, _))) => {
                let entry = create_synced_commit_mapping_entry(self.cs_id, *new_cs_id, &self.repos);
                Ok(Box::new(CrossRepoSyncTransactionHook { entry })
                    as Box<dyn PushrebaseTransactionHook>)
            }
            None => {
                return Err(format_err!("expected exactly one commit to be rebased").into());
            }
        }
    }
}

#[derive(Clone)]
pub struct CrossRepoSyncTransactionHook {
    entry: SyncedCommitMappingEntry,
}

#[async_trait]
impl PushrebaseTransactionHook for CrossRepoSyncTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let (txn, _) = add_many_in_txn(txn, vec![self.entry.clone()])
            .compat()
            .await?;
        Ok(txn)
    }
}
