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
use synced_commit_mapping::add_many_in_txn;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;

use crate::commit_syncers_lib::create_synced_commit_mapping_entry;
use crate::types::Repo;
use crate::CommitSyncRepos;
use crate::ErrorKind;

/// Structure present if given pushrebase is a forward (small-to-large) sync and mapping should be
/// updated accordingly.
#[derive(Clone)]
pub struct ForwardSyncedCommitInfo<R> {
    pub cs_id: ChangesetId,
    pub repos: CommitSyncRepos<R>,
    pub version_name: CommitSyncConfigVersion,
}

pub struct CrossRepoSyncPushrebaseHook<R, M> {
    #[allow(dead_code)]
    synced_commit_mapping: M,
    forward_synced_commit_info: Option<ForwardSyncedCommitInfo<R>>,
}

impl<R: Repo + 'static, M: SyncedCommitMapping + Clone + 'static>
    CrossRepoSyncPushrebaseHook<R, M>
{
    pub fn new(
        synced_commit_mapping: M,
        forward_synced_commit_info: Option<ForwardSyncedCommitInfo<R>>,
    ) -> Box<dyn PushrebaseHook> {
        Box::new(Self {
            synced_commit_mapping,
            forward_synced_commit_info,
        })
    }
}

#[async_trait]
impl<R: Repo + 'static, M: SyncedCommitMapping + Clone + 'static> PushrebaseHook
    for CrossRepoSyncPushrebaseHook<R, M>
{
    async fn in_critical_section(
        &self,
        _ctx: &CoreContext,
        _old_bookmark_value: Option<ChangesetId>,
    ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let hook = Box::new(CrossRepoSyncPushrebaseCommitHook {
            forward_synced_commit_info: self.forward_synced_commit_info.clone(),
        }) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

#[derive(Clone)]
pub struct CrossRepoSyncPushrebaseCommitHook<R> {
    forward_synced_commit_info: Option<ForwardSyncedCommitInfo<R>>,
}

#[async_trait]
impl<R: Repo + 'static> PushrebaseCommitHook for CrossRepoSyncPushrebaseCommitHook<R> {
    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        if let Some(forward_synced_commit_info) = self.forward_synced_commit_info {
            if rebased.len() > 1 {
                return Err(format_err!("expected exactly one commit to be rebased"));
            }

            match rebased.iter().next() {
                Some((_, (new_cs_id, _))) => {
                    let entry = create_synced_commit_mapping_entry(
                        forward_synced_commit_info.cs_id,
                        *new_cs_id,
                        &forward_synced_commit_info.repos,
                        forward_synced_commit_info.version_name.clone(),
                    );
                    Ok(Box::new(CrossRepoSyncTransactionHook {
                        forward_synced_entry: Some(entry),
                    }) as Box<dyn PushrebaseTransactionHook>)
                }
                None => {
                    return Err(format_err!("expected exactly one commit to be rebased"));
                }
            }
        } else {
            Ok(Box::new(CrossRepoSyncTransactionHook {
                forward_synced_entry: None,
            }) as Box<dyn PushrebaseTransactionHook>)
        }
    }
}

#[derive(Clone)]
pub struct CrossRepoSyncTransactionHook {
    forward_synced_entry: Option<SyncedCommitMappingEntry>,
}

#[async_trait]
impl PushrebaseTransactionHook for CrossRepoSyncTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        if let Some(entry) = &self.forward_synced_entry {
            let xrepo_sync_disable_all_syncs =
                justknobs::eval("scm/mononoke:xrepo_sync_disable_all_syncs", None, None)
                    .unwrap_or_default();
            if xrepo_sync_disable_all_syncs {
                let e: Error = ErrorKind::XRepoSyncDisabled.into();
                return Err(e.into());
            }
            let (txn, _) = add_many_in_txn(txn, vec![entry.clone()]).await?;
            Ok(txn)
        } else {
            Ok(txn)
        }
    }
}
