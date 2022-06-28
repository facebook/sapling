/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use mononoke_types::hash::GitSha1;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use pushrebase_hook::PushrebaseCommitHook;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hook::PushrebaseTransactionHook;
use pushrebase_hook::RebasedChangesets;
use sql::Transaction;
use std::collections::HashMap;
use std::sync::Arc;

use bonsai_git_mapping::extract_git_sha1_from_bonsai_extra;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;

#[cfg(test)]
mod test;

#[derive(Clone)]
pub struct GitMappingPushrebaseHook {
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
}

impl GitMappingPushrebaseHook {
    pub fn new(bonsai_git_mapping: Arc<dyn BonsaiGitMapping>) -> Box<dyn PushrebaseHook> {
        Box::new(Self { bonsai_git_mapping })
    }
}

#[async_trait]
impl PushrebaseHook for GitMappingPushrebaseHook {
    async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(GitMappingCommitHook {
            bonsai_git_mapping: self.bonsai_git_mapping.clone(),
            assignments: HashMap::new(),
        }) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

struct GitMappingCommitHook {
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    assignments: HashMap<ChangesetId, GitSha1>,
}

#[async_trait]
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

    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
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

                Ok(BonsaiGitMappingEntry::new(*git_sha1, replacement_bcs_id))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        // NOTE: This check shouldn't be necessary as long as pushrebase hooks are bug-free, but
        // since they're a new addition, let's be conservative.
        if rebased.len() != self.assignments.len() {
            return Err(anyhow!(
                "Git mapping rebased set ({}) and assignments ({}) have different lengths!",
                rebased.len(),
                self.assignments.len(),
            ));
        }

        Ok(Box::new(GitMappingTransactionHook {
            bonsai_git_mapping: self.bonsai_git_mapping,
            entries,
        }) as Box<dyn PushrebaseTransactionHook>)
    }
}

struct GitMappingTransactionHook {
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    entries: Vec<BonsaiGitMappingEntry>,
}

#[async_trait]
impl PushrebaseTransactionHook for GitMappingTransactionHook {
    async fn populate_transaction(
        &self,
        ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let txn = self
            .bonsai_git_mapping
            .bulk_add_git_mapping_in_transaction(ctx, &self.entries[..], txn)
            .await
            .map_err(|e| BookmarkTransactionError::Other(e.into()))?;
        Ok(txn)
    }
}
