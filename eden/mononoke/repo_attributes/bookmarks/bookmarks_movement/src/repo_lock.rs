/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use permission_checker::MononokeIdentitySet;
use pushrebase::RepoLockPolicy;
use repo_authorization::AuthorizationContext;
use repo_lock::RepoLockRef;
use repo_lock::RepoLockState;
use repo_permission_checker::RepoPermissionChecker;
use repo_permission_checker::RepoPermissionCheckerRef;

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

                    if !bypass_allowed {
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
    ctx: &CoreContext,
    repo: &(impl RepoLockRef + RepoPermissionCheckerRef),
    kind: BookmarkKind,
    pushvars: Option<&HashMap<String, Bytes>>,
    idents: &MononokeIdentitySet,
    authz: &AuthorizationContext,
) -> Result<RepoLockPolicy, BookmarkMovementError> {
    let should_check = should_check_repo_lock(
        kind,
        pushvars,
        repo.repo_permission_checker(),
        idents,
        authz,
    )
    .await;
    if should_check {
        let state = repo
            .repo_lock()
            .check_repo_lock(ctx)
            .await
            .context("Failed to fetch repo lock state")?;

        if let RepoLockState::Locked(reason) = state {
            return Err(BookmarkMovementError::RepoLocked(reason));
        }
    }

    Ok(if should_check {
        RepoLockPolicy::Enforce
    } else {
        RepoLockPolicy::Bypass
    })
}
