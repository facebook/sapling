/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use ::repo_lock::RepoLockRef;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingArc;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarksRef;
use bookmarks_types::BookmarkKey;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use itertools::Itertools;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use phases::PhasesRef;
use pushrebase::PushrebaseError;
use pushrebase_mutation_mapping::PushrebaseMutationMappingRef;
use repo_authorization::AuthorizationError;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrsRef;
use repo_cross_repo::RepoCrossRepoRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use repo_permission_checker::RepoPermissionCheckerRef;
use repo_update_logger::log_bookmark_operation;
use repo_update_logger::log_new_bonsai_changesets;
use repo_update_logger::BookmarkInfo;
use thiserror::Error;

pub mod affected_changesets;
mod create;
mod delete;
mod git_mapping;
mod hook_running;
mod pushrebase_onto;
mod repo_lock;
mod restrictions;
mod update;

pub use bookmarks_types::BookmarkKind;
use git_source_of_truth::GitSourceOfTruthConfigRef;
pub use hooks::CrossRepoPushSource;
pub use hooks::HookRejection;
pub use pushrebase::PushrebaseOutcome;
pub use pushrebase_hooks::get_pushrebase_hooks;
pub use pushrebase_hooks::PushrebaseHooksError;

pub use crate::create::CreateBookmarkOp;
pub use crate::delete::DeleteBookmarkOp;
pub use crate::hook_running::run_bookmark_hooks;
pub use crate::hook_running::run_changeset_hooks;
pub use crate::pushrebase_onto::PushrebaseOntoBookmarkOp;
pub use crate::restrictions::check_bookmark_sync_config;
pub use crate::restrictions::BookmarkKindRestrictions;
pub use crate::update::BookmarkUpdatePolicy;
pub use crate::update::BookmarkUpdateTargets;
pub use crate::update::UpdateBookmarkOp;

const ALLOW_NON_FFWD_PUSHVAR: &str = "x-git-allow-non-ffwd-push";

/// Trait alias for bookmarks movement repositories.
///
/// These are the repo attributes that are necessary to call most functions in
/// bookmarks movement.
pub trait Repo = BonsaiHgMappingRef
    + BonsaiGitMappingArc
    + BonsaiGlobalrevMappingArc
    + BookmarksRef
    + PhasesRef
    + PushrebaseMutationMappingRef
    + RepoBookmarkAttrsRef
    + RepoConfigRef
    + RepoDerivedDataRef
    + RepoBlobstoreArc
    + RepoBlobstoreRef
    + RepoCrossRepoRef
    + RepoIdentityRef
    + RepoPermissionCheckerRef
    + RepoLockRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + GitSourceOfTruthConfigRef
    + Send
    + Sync;

/// An error encountered during an attempt to move a bookmark.
#[derive(Debug, Error)]
pub enum BookmarkMovementError {
    #[error("Non fast-forward bookmark move of '{bookmark}' from {from} to {to}")]
    NonFastForwardMove {
        bookmark: BookmarkKey,
        from: ChangesetId,
        to: ChangesetId,
    },

    #[error("Deletion of '{bookmark}' is prohibited")]
    DeletionProhibited { bookmark: BookmarkKey },

    #[error(transparent)]
    AuthorizationError(#[from] AuthorizationError),

    #[error(
        "Invalid scratch bookmark: {bookmark} (scratch bookmarks must match pattern {pattern})"
    )]
    InvalidScratchBookmark {
        bookmark: BookmarkKey,
        pattern: String,
    },

    #[error(
        "Invalid publishing bookmark: {bookmark} (only scratch bookmarks may match pattern {pattern})"
    )]
    InvalidPublishingBookmark {
        bookmark: BookmarkKey,
        pattern: String,
    },

    #[error(
        "Invalid scratch bookmark: {bookmark} (scratch bookmarks are not enabled for this repo)"
    )]
    ScratchBookmarksDisabled { bookmark: BookmarkKey },

    #[error("Bookmark transaction failed")]
    TransactionFailed,

    #[error("Hooks failed:\n{}", describe_hook_rejections(.0.as_slice()))]
    HookFailure(Vec<HookRejection>),

    #[error("Pushrebase failed: {0}")]
    PushrebaseError(#[source] PushrebaseError),

    #[error(transparent)]
    PushrebaseHooksError(#[from] PushrebaseHooksError),

    #[error("Repo is locked: {0}")]
    RepoLocked(String),

    #[error("Case conflict found in {changeset_id}: {path1} conflicts with {path2}")]
    CaseConflict {
        changeset_id: ChangesetId,
        path1: NonRootMPath,
        path2: NonRootMPath,
    },

    #[error("Bookmark '{bookmark}' can only be moved to ancestors of '{descendant_bookmark}'")]
    RequiresAncestorOf {
        bookmark: BookmarkKey,
        descendant_bookmark: BookmarkKey,
    },

    #[error(
        "Bookmark '{bookmark}' cannot be moved because publishing bookmarks are being redirected"
    )]
    PushRedirectorEnabledForPublishing { bookmark: BookmarkKey },

    #[error("Bookmark '{bookmark}' cannot be moved because scratch bookmarks are being redirected")]
    PushRedirectorEnabledForScratch { bookmark: BookmarkKey },

    #[error(transparent)]
    Error(#[from] anyhow::Error),
}

pub fn describe_hook_rejections(rejections: &[HookRejection]) -> String {
    rejections
        .iter()
        .map(|rejection| {
            format!(
                "  {} for {}: {}",
                rejection.hook_name, rejection.cs_id, rejection.reason.long_description
            )
        })
        .join("\n")
}

pub struct BookmarkInfoData {
    bookmark_info: BookmarkInfo,
    log_new_public_commits_to_scribe: bool,
    commits: Vec<BonsaiChangeset>,
}

impl BookmarkInfoData {
    pub fn new(
        bookmark_info: BookmarkInfo,
        log_new_public_commits_to_scribe: bool,
        commits: Vec<BonsaiChangeset>,
    ) -> Self {
        Self {
            bookmark_info,
            log_new_public_commits_to_scribe,
            commits,
        }
    }

    pub async fn log(
        self,
        ctx: &CoreContext,
        repo: &(
             impl RepoIdentityRef
             + CommitGraphRef
             + RepoConfigRef
             + BonsaiGlobalrevMappingRef
             + BonsaiGitMappingRef
             + GitSourceOfTruthConfigRef
         ),
    ) {
        if self.log_new_public_commits_to_scribe {
            log_new_bonsai_changesets(
                ctx,
                repo,
                &self.bookmark_info.bookmark_name,
                self.bookmark_info.bookmark_kind,
                self.commits,
            )
            .await;
        }
        log_bookmark_operation(ctx, repo, &self.bookmark_info).await;
    }
}

pub struct TransactionWithHooks {
    pub transaction: Box<dyn BookmarkTransaction>,
    pub txn_hooks: Vec<BookmarkTransactionHook>,
}

impl TransactionWithHooks {
    pub fn new(
        transaction: Box<dyn BookmarkTransaction>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Self {
        Self {
            transaction,
            txn_hooks,
        }
    }

    pub async fn commit(self) -> Result<BookmarkUpdateLogId, BookmarkMovementError> {
        let maybe_log_id = self.transaction.commit_with_hooks(self.txn_hooks).await?;
        if let Some(log_id) = maybe_log_id {
            Ok(log_id.into())
        } else {
            Err(BookmarkMovementError::TransactionFailed)
        }
    }
}

pub struct BookmarkInfoTransaction {
    pub info_data: BookmarkInfoData,
    pub transaction: TransactionWithHooks,
}

impl BookmarkInfoTransaction {
    pub fn new(
        info_data: BookmarkInfoData,
        transaction: Box<dyn BookmarkTransaction>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Self {
        Self {
            info_data,
            transaction: TransactionWithHooks::new(transaction, txn_hooks),
        }
    }

    pub async fn commit_and_log(
        self,
        ctx: &CoreContext,
        repo: &impl Repo,
    ) -> Result<BookmarkUpdateLogId, BookmarkMovementError> {
        let log_id = self.transaction.commit().await?;
        self.info_data.log(ctx, repo).await;
        Ok(log_id)
    }
}
