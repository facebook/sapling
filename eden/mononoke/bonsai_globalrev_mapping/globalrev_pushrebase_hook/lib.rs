/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use mononoke_types::{
    globalrev::{Globalrev, GLOBALREV_EXTRA, START_COMMIT_GLOBALREV},
    BonsaiChangesetMut, ChangesetId, RepositoryId,
};
use pushrebase_hook::{
    PushrebaseCommitHook, PushrebaseHook, PushrebaseTransactionHook, RebasedChangesets,
};
use sql::Transaction;
use std::collections::HashMap;
use std::sync::Arc;

use bonsai_globalrev_mapping::{
    add_globalrevs, AddGlobalrevsErrorKind, BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry,
};

#[cfg(test)]
mod test;

#[derive(Clone)]
pub struct GlobalrevPushrebaseHook {
    ctx: CoreContext,
    mapping: Arc<dyn BonsaiGlobalrevMapping>,
    repository_id: RepositoryId,
}

impl GlobalrevPushrebaseHook {
    pub fn new(
        ctx: CoreContext,
        mapping: Arc<dyn BonsaiGlobalrevMapping>,
        repository_id: RepositoryId,
    ) -> Box<dyn PushrebaseHook> {
        Box::new(Self {
            ctx,
            mapping,
            repository_id,
        })
    }
}

#[async_trait]
impl PushrebaseHook for GlobalrevPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let max = self.mapping.get_max(&self.ctx).await?;

        let next_rev = match max {
            None => START_COMMIT_GLOBALREV,
            Some(max) => max.id() + 1,
        };

        let hook = Box::new(GlobalrevCommitHook {
            repository_id: self.repository_id,
            assignments: HashMap::new(),
            next_rev,
        }) as Box<dyn PushrebaseCommitHook>;

        Ok(hook)
    }
}

struct GlobalrevCommitHook {
    repository_id: RepositoryId,
    assignments: HashMap<ChangesetId, Globalrev>,
    next_rev: u64,
}

#[async_trait]
impl PushrebaseCommitHook for GlobalrevCommitHook {
    fn post_rebase_changeset(
        &mut self,
        bcs_old: ChangesetId,
        bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        bcs_new.extra.insert(
            GLOBALREV_EXTRA.into(),
            format!("{}", self.next_rev).into_bytes(),
        );

        self.assignments
            .insert(bcs_old, Globalrev::new(self.next_rev));

        self.next_rev += 1;

        Ok(())
    }

    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        // Let's tie assigned globalrevs to rebased Bonsai changesets:
        let entries = self
            .assignments
            .iter()
            .map(|(cs_id, globalrev)| {
                let replacement_bcs_id = rebased
                    .get(cs_id)
                    .ok_or_else(|| {
                        let e = format!(
                            "Commit was assigned a Globalrev, but is not found in rebased set: {}",
                            cs_id
                        );
                        Error::msg(e)
                    })?
                    .0;

                Ok(BonsaiGlobalrevMappingEntry::new(
                    replacement_bcs_id,
                    *globalrev,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        // NOTE: This check shouldn't be necessary as long as pushrebase hooks are bug-free, but
        // since they're a new addition, let's be conservative.
        if rebased.len() != self.assignments.len() {
            return Err(anyhow!(
                "Globalrev rebased set ({}) and assignments ({}) have different lengths!",
                rebased.len(),
                self.assignments.len(),
            ));
        }

        Ok(Box::new(GlobalrevTransactionHook {
            repo_id: self.repository_id,
            entries,
        }) as Box<dyn PushrebaseTransactionHook>)
    }
}

struct GlobalrevTransactionHook {
    repo_id: RepositoryId,
    entries: Vec<BonsaiGlobalrevMappingEntry>,
}

#[async_trait]
impl PushrebaseTransactionHook for GlobalrevTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let txn = add_globalrevs(txn, self.repo_id, &self.entries[..])
            .await
            .map_err(|e| match e {
                AddGlobalrevsErrorKind::Conflict => BookmarkTransactionError::LogicError,
                e @ AddGlobalrevsErrorKind::InternalError(..) => {
                    BookmarkTransactionError::Other(e.into())
                }
            })?;

        Ok(txn)
    }
}
