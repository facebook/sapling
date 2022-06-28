/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod test;

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use pushrebase_hook::PushrebaseCommitHook;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hook::PushrebaseTransactionHook;
use pushrebase_hook::RebasedChangesets;
use sql::Transaction;

use crate::sql_queries::add_pushrebase_mapping;
use crate::PushrebaseMutationMappingEntry;

pub struct SaveMappingPushrebaseHook {
    repository_id: RepositoryId,
}

impl SaveMappingPushrebaseHook {
    pub fn new(repository_id: RepositoryId) -> Box<dyn PushrebaseHook> {
        Box::new(Self { repository_id })
    }
}

#[async_trait]
impl PushrebaseHook for SaveMappingPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>> {
        Ok(Box::new(SaveMappingCommitHook {
            repository_id: self.repository_id,
        }))
    }
}

pub struct SaveMappingCommitHook {
    repository_id: RepositoryId,
}

#[async_trait]
impl PushrebaseCommitHook for SaveMappingCommitHook {
    fn post_rebase_changeset(
        &mut self,
        _bcs_old: ChangesetId,
        _bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<()> {
        Ok(())
    }

    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>> {
        let entries = rebased
            .iter()
            .map(|(predecessor_bcs_id, (successor_bcs_id, _))| {
                PushrebaseMutationMappingEntry::new(
                    self.repository_id,
                    *predecessor_bcs_id,
                    *successor_bcs_id,
                )
            })
            .collect();
        Ok(Box::new(SaveMappingTransactionHook { entries }))
    }
}

struct SaveMappingTransactionHook {
    entries: Vec<PushrebaseMutationMappingEntry>,
}

#[async_trait]
impl PushrebaseTransactionHook for SaveMappingTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let txn = add_pushrebase_mapping(txn, &self.entries[..]).await?;
        Ok(txn)
    }
}
