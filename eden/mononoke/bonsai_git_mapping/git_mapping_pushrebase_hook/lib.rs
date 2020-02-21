/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use mononoke_types::{hash::GitSha1, BonsaiChangesetMut, ChangesetId, RepositoryId};
use pushrebase::{
    PushrebaseCommitHook, PushrebaseHook, PushrebaseTransactionHook, RebasedChangesets,
};
use sql::Transaction;
use std::collections::HashMap;

use bonsai_git_mapping::{
    bulk_add_git_mapping_in_transaction, extract_git_sha1_from_bonsai_extra, BonsaiGitMappingEntry,
};

#[cfg(test)]
mod test;

#[derive(Clone)]
pub struct GitMappingPushrebaseHook {
    repository_id: RepositoryId,
}

impl GitMappingPushrebaseHook {
    pub fn new(repository_id: RepositoryId) -> Box<dyn PushrebaseHook> {
        Box::new(Self { repository_id })
    }
}

#[async_trait]
impl PushrebaseHook for GitMappingPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(GitMappingCommitHook {
            repository_id: self.repository_id,
            assignments: HashMap::new(),
        }) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

struct GitMappingCommitHook {
    repository_id: RepositoryId,
    assignments: HashMap<ChangesetId, GitSha1>,
}

impl PushrebaseCommitHook for GitMappingCommitHook {
    fn post_rebase_changeset(
        &mut self,
        bcs_old: ChangesetId,
        bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        let git_sha1 = extract_git_sha1_from_bonsai_extra(
            bcs_new
                .extra
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_slice())),
        )?;
        if let Some(git_sha1) = git_sha1 {
            self.assignments.insert(bcs_old, git_sha1);
        }
        Ok(())
    }

    fn into_transaction_hook(
        self: Box<Self>,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        // Let's tie assigned git hashes to rebased Bonsai changesets:
        let entries = self
            .assignments
            .iter()
            .map(|(cs_id, git_sha1)| {
                let replacement_bcs_id = rebased
                    .get(cs_id)
                    .ok_or_else(|| {
                        let e = format!(
                            "Commit was assigned a git hash, but is not found in rebased set: {}",
                            cs_id
                        );
                        Error::msg(e)
                    })?
                    .0;

                Ok(BonsaiGitMappingEntry::new(
                    self.repository_id,
                    *git_sha1,
                    replacement_bcs_id,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        // NOTE: This check shouldn't be necessary as long as pushrebase hooks are bug-free, but
        // since they're a new addition, let's be conservative.
        if rebased.len() != self.assignments.len() {
            return Err(Error::msg(
                "Rebased set and assignments have different lengths!",
            ));
        }

        Ok(Box::new(GitMappingTransactionHook { entries }) as Box<dyn PushrebaseTransactionHook>)
    }
}

struct GitMappingTransactionHook {
    entries: Vec<BonsaiGitMappingEntry>,
}

#[async_trait]
impl PushrebaseTransactionHook for GitMappingTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let txn = bulk_add_git_mapping_in_transaction(txn, &self.entries[..])
            .await
            .map_err(|e| BookmarkTransactionError::Other(e.into()))?;
        Ok(txn)
    }
}
