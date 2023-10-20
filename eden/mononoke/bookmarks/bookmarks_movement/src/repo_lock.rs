/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use permission_checker::MononokeIdentitySet;
use pushrebase_hook::PushrebaseCommitHook;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hook::PushrebaseTransactionHook;
use pushrebase_hook::RebasedChangesets;
use repo_authorization::AuthorizationContext;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;
use repo_lock::TransactionRepoLock;
use repo_permission_checker::RepoPermissionChecker;
use repo_permission_checker::RepoPermissionCheckerRef;
use sql::Transaction;
use tunables::tunables;

use crate::BookmarkMovementError;

async fn should_check_repo_lock(
    kind: BookmarkKind,
    pushvars: Option<&HashMap<String, Bytes>>,
    repo_perm_checker: &dyn RepoPermissionChecker,
    idents: &MononokeIdentitySet,
    authz: &AuthorizationContext,
) -> bool {
    match kind {
        BookmarkKind::Scratch => false,
        BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
            if let Some(pushvars) = pushvars {
                if let Some(value) = pushvars.get("BYPASS_READONLY") {
                    let mut bypass_allowed = repo_perm_checker
                        .check_if_read_only_bypass_allowed(idents)
                        .await;
                    // If this operation is executing in an internal admin-only context (e.g. gitimport)
                    // then we allow it to bypass repo lock check
                    bypass_allowed |= authz == &AuthorizationContext::FullAccess;

                    let enforce_acl_check =
                        tunables().enforce_bypass_readonly_acl().unwrap_or_default();

                    if !bypass_allowed && enforce_acl_check {
                        return true;
                    }

                    if value.to_ascii_lowercase() == b"true" {
                        return false;
                    }
                }
            }
            true
        }
    }
}

pub(crate) async fn check_repo_lock(
    repo: &(impl RepoLockRef + RepoPermissionCheckerRef),
    kind: BookmarkKind,
    pushvars: Option<&HashMap<String, Bytes>>,
    idents: &MononokeIdentitySet,
    authz: &AuthorizationContext,
) -> Result<(), BookmarkMovementError> {
    if should_check_repo_lock(
        kind,
        pushvars,
        repo.repo_permission_checker(),
        idents,
        authz,
    )
    .await
    {
        let state = repo
            .repo_lock()
            .check_repo_lock()
            .await
            .context("Failed to fetch repo lock state")?;

        if let RepoLockState::Locked(reason) = state {
            return Err(BookmarkMovementError::RepoLocked(reason));
        }
    }

    Ok(())
}

pub(crate) struct RepoLockPushrebaseHook {
    transaction_repo_lock: TransactionRepoLock,
}

impl RepoLockPushrebaseHook {
    pub(crate) async fn new(
        repo_id: RepositoryId,
        kind: BookmarkKind,
        pushvars: Option<&HashMap<String, Bytes>>,
        repo_perm_checker: &dyn RepoPermissionChecker,
        idents: &MononokeIdentitySet,
        authz: &AuthorizationContext,
    ) -> Option<Box<dyn PushrebaseHook>> {
        if should_check_repo_lock(kind, pushvars, repo_perm_checker, idents, authz).await {
            let hook = Box::new(RepoLockPushrebaseHook {
                transaction_repo_lock: TransactionRepoLock::new(repo_id),
            });
            Some(hook as Box<dyn PushrebaseHook>)
        } else {
            None
        }
    }
}

#[async_trait]
impl PushrebaseHook for RepoLockPushrebaseHook {
    async fn in_critical_section(&self) -> Result<Box<dyn PushrebaseCommitHook>> {
        let hook = Box::new(RepoLockCommitTransactionHook {
            transaction_repo_lock: self.transaction_repo_lock,
        });
        Ok(hook as Box<dyn PushrebaseCommitHook>)
    }
}

struct RepoLockCommitTransactionHook {
    transaction_repo_lock: TransactionRepoLock,
}

#[async_trait]
impl PushrebaseCommitHook for RepoLockCommitTransactionHook {
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
        _rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>> {
        Ok(self as Box<dyn PushrebaseTransactionHook>)
    }
}

#[async_trait]
impl PushrebaseTransactionHook for RepoLockCommitTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let (txn, state) = self
            .transaction_repo_lock
            .check_repo_lock_with_transaction(txn)
            .await
            .context("Failed to fetch repo lock state")?;
        if let RepoLockState::Locked(reason) = state {
            return Err(BookmarkTransactionError::Other(anyhow!(
                "Repo is locked: {}",
                reason
            )));
        }

        Ok(txn)
    }
}
