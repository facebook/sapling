/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::pushrebase_hook::PushrebaseCommitHook;
use ::pushrebase_hook::PushrebaseHook;
use ::pushrebase_hook::PushrebaseTransactionHook;
use ::pushrebase_hook::RebasedChangesets;
use anyhow::format_err;
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use sql::Transaction;
use tunables::tunables;

use crate::create_synced_commit_mapping_entry;
use crate::CommitSyncRepos;
use crate::ErrorKind;
use synced_commit_mapping::add_many_in_txn;
use synced_commit_mapping::SyncedCommitMappingEntry;

#[derive(Clone)]
pub struct CrossRepoSyncPushrebaseHook {
    cs_id: ChangesetId,
    repos: CommitSyncRepos,
    version_name: CommitSyncConfigVersion,
}

impl CrossRepoSyncPushrebaseHook {
    pub fn new(
        cs_id: ChangesetId,
        repos: CommitSyncRepos,
        version_name: CommitSyncConfigVersion,
    ) -> Box<dyn PushrebaseHook> {
        Box::new(Self {
            cs_id,
            repos,
            version_name,
        })
    }
}

#[async_trait]
impl PushrebaseHook for CrossRepoSyncPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(self.clone()) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

#[async_trait]
impl PushrebaseCommitHook for CrossRepoSyncPushrebaseHook {
    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        if rebased.len() > 1 {
            return Err(format_err!("expected exactly one commit to be rebased"));
        }

        match rebased.iter().next() {
            Some((_, (new_cs_id, _))) => {
                let entry = create_synced_commit_mapping_entry(
                    self.cs_id,
                    *new_cs_id,
                    &self.repos,
                    self.version_name.clone(),
                );
                Ok(Box::new(CrossRepoSyncTransactionHook { entry })
                    as Box<dyn PushrebaseTransactionHook>)
            }
            None => {
                return Err(format_err!("expected exactly one commit to be rebased"));
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
        if tunables().get_xrepo_sync_disable_all_syncs() {
            let e: Error = ErrorKind::XRepoSyncDisabled.into();
            return Err(e.into());
        }
        let (txn, _) = add_many_in_txn(txn, vec![self.entry.clone()]).await?;
        Ok(txn)
    }
}
