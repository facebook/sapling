/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use bookmarks_types::BookmarkName;
use itertools::Itertools;
use mononoke_types::{ChangesetId, MPath};
use pushrebase::PushrebaseError;
use thiserror::Error;

mod affected_changesets;
mod create;
mod delete;
#[cfg(fbcode_build)]
mod facebook;
mod git_mapping;
mod globalrev_mapping;
mod hook_running;
mod pushrebase_onto;
mod repo_lock;
mod restrictions;
mod update;

pub use hooks::{CrossRepoPushSource, HookRejection};
pub use pushrebase::PushrebaseOutcome;

pub use crate::affected_changesets::log_commits_to_scribe;
pub use crate::create::CreateBookmarkOp;
pub use crate::delete::DeleteBookmarkOp;
pub use crate::hook_running::run_hooks;
pub use crate::pushrebase_onto::PushrebaseOntoBookmarkOp;
pub use crate::update::{BookmarkUpdatePolicy, BookmarkUpdateTargets, UpdateBookmarkOp};

/// An error encountered during an attempt to move a bookmark.
#[derive(Debug, Error)]
pub enum BookmarkMovementError {
    #[error("Non fast-forward bookmark move from {from} to {to}")]
    NonFastForwardMove { from: ChangesetId, to: ChangesetId },

    #[error("Pushrebase required when assigning globalrevs")]
    PushrebaseRequiredGlobalrevs,

    #[error("Deletion of '{bookmark}' is prohibited")]
    DeletionProhibited { bookmark: BookmarkName },

    #[error("User '{user}' is not permitted to move '{bookmark}'")]
    PermissionDeniedUser {
        user: String,
        bookmark: BookmarkName,
    },

    #[error("Service '{service_name}' is not permitted to move '{bookmark}'")]
    PermissionDeniedServiceBookmark {
        service_name: String,
        bookmark: BookmarkName,
    },

    #[error("Service '{service_name}' is not permitted to modify path '{path}'")]
    PermissionDeniedServicePath { service_name: String, path: MPath },

    #[error(
        "Invalid scratch bookmark: {bookmark} (scratch bookmarks must match pattern {pattern})"
    )]
    InvalidScratchBookmark {
        bookmark: BookmarkName,
        pattern: String,
    },

    #[error(
        "Invalid public bookmark: {bookmark} (only scratch bookmarks may match pattern {pattern})"
    )]
    InvalidPublicBookmark {
        bookmark: BookmarkName,
        pattern: String,
    },

    #[error(
        "Invalid scratch bookmark: {bookmark} (scratch bookmarks are not enabled for this repo)"
    )]
    ScratchBookmarksDisabled { bookmark: BookmarkName },

    #[error("Bookmark transaction failed")]
    TransactionFailed,

    #[error("Hooks failed:\n{}", describe_hook_rejections(.0.as_slice()))]
    HookFailure(Vec<HookRejection>),

    #[error("Pushrebase failed: {0}")]
    PushrebaseError(#[source] PushrebaseError),

    #[error("Repo is locked: {0}")]
    RepoLocked(String),

    #[error("Case conflict found in {changeset_id}: {path1} conflicts with {path2}")]
    CaseConflict {
        changeset_id: ChangesetId,
        path1: MPath,
        path2: MPath,
    },

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
