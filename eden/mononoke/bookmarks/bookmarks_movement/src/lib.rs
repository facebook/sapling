/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(unused)]

use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams};
use mononoke_types::ChangesetId;
use thiserror::Error;

mod create;
mod delete;
mod git_mapping;
mod globalrev_mapping;
mod update;

pub use crate::create::CreateBookmarkOp;
pub use crate::delete::DeleteBookmarkOp;
pub use crate::update::{BookmarkUpdatePolicy, BookmarkUpdateTargets, UpdateBookmarkOp};

/// How authorization for the bookmark move should be determined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkMoveAuthorization {
    /// Use identity information in the core context.
    Context,
}

impl BookmarkMoveAuthorization {
    fn check_authorized(
        &self,
        ctx: &CoreContext,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
    ) -> Result<(), BookmarkMovementError> {
        match self {
            BookmarkMoveAuthorization::Context => {
                if let Some(user) = ctx.user_unix_name() {
                    // TODO: clean up `is_allowed_user` to avoid this clone.
                    if !bookmark_attrs.is_allowed_user(&Some(user.clone()), bookmark) {
                        return Err(BookmarkMovementError::PermissionDeniedUser {
                            user: user.clone(),
                            bookmark: bookmark.clone(),
                        });
                    }
                }
                // TODO: Check using ctx.identities, and deny if neither are provided.
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BookmarkKindRestrictions {
    AnyKind,
    OnlyScratch,
    OnlyPublic,
}

impl BookmarkKindRestrictions {
    fn check_kind(
        &self,
        infinitepush_params: &InfinitepushParams,
        name: &BookmarkName,
    ) -> Result<bool, BookmarkMovementError> {
        match (self, &infinitepush_params.namespace) {
            (Self::OnlyScratch, None) => Err(BookmarkMovementError::ScratchBookmarksDisabled {
                bookmark: name.clone(),
            }),
            (Self::OnlyScratch, Some(namespace)) if !namespace.matches_bookmark(name) => {
                Err(BookmarkMovementError::InvalidScratchBookmark {
                    bookmark: name.clone(),
                    pattern: namespace.as_str().to_string(),
                })
            }
            (Self::OnlyPublic, Some(namespace)) if namespace.matches_bookmark(name) => {
                Err(BookmarkMovementError::InvalidPublicBookmark {
                    bookmark: name.clone(),
                    pattern: namespace.as_str().to_string(),
                })
            }
            (_, Some(namespace)) => Ok(namespace.matches_bookmark(name)),
            (_, None) => Ok(false),
        }
    }
}

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

    #[error(transparent)]
    Error(#[from] anyhow::Error),
}
