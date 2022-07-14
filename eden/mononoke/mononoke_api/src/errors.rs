/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blame::BlameError;
use blobstore::LoadableError;
use bookmarks_movement::describe_hook_rejections;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::HookRejection;
use derived_data::DeriveError;
use itertools::Itertools;
use megarepo_error::MegarepoError;
use pushrebase::PushrebaseError;
use repo_authorization::AuthorizationError;
use std::backtrace::Backtrace;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use crate::path::MononokePath;

use anyhow::Error;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct InternalError(Arc<Error>);

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Error> for InternalError {
    fn from(error: Error) -> Self {
        Self(Arc::new(error))
    }
}

impl StdError for InternalError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&**self.0)
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        Some(self.0.backtrace())
    }
}

#[derive(Clone, Debug, Error)]
pub enum MononokeError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("unresolved path conflicts in merge:\n {}", .conflict_paths.iter().join("\n"))]
    MergeConflicts { conflict_paths: Vec<MononokePath> },
    #[error("Conflicts while pushrebasing: {0:?}")]
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    #[error(
        "permission denied: access to repo {reponame} on behalf of {service_identity} not permitted for {identities}"
    )]
    ServicePermissionDenied {
        identities: String,
        reponame: String,
        service_identity: String,
    },
    #[error("hooks failed:\n{}", describe_hook_rejections(.0.as_slice()))]
    HookFailure(Vec<HookRejection>),
    #[error("not available: {0}")]
    NotAvailable(String),
    #[error("permission denied: {0}")]
    AuthorizationError(String),
    #[error("internal error: {0}")]
    InternalError(#[source] InternalError),
}

impl From<Error> for MononokeError {
    fn from(e: Error) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e)))
    }
}

impl From<Infallible> for MononokeError {
    fn from(_i: Infallible) -> Self {
        unreachable!()
    }
}

impl From<LoadableError> for MononokeError {
    fn from(e: LoadableError) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e.into())))
    }
}

impl From<DeriveError> for MononokeError {
    fn from(e: DeriveError) -> Self {
        match e {
            e @ DeriveError::Disabled(..) => MononokeError::NotAvailable(e.to_string()),
            DeriveError::Error(e) => MononokeError::from(e),
        }
    }
}

impl From<BookmarkMovementError> for MononokeError {
    fn from(e: BookmarkMovementError) -> Self {
        match e {
            BookmarkMovementError::AuthorizationError(e) => {
                MononokeError::AuthorizationError(e.to_string())
            }
            BookmarkMovementError::HookFailure(rejections) => {
                MononokeError::HookFailure(rejections)
            }
            BookmarkMovementError::PushrebaseError(PushrebaseError::Conflicts(conflicts)) => {
                MononokeError::PushrebaseConflicts(conflicts)
            }
            BookmarkMovementError::Error(e) => MononokeError::InternalError(InternalError::from(e)),
            _ => MononokeError::InvalidRequest(e.to_string()),
        }
    }
}

impl From<AuthorizationError> for MononokeError {
    fn from(e: AuthorizationError) -> Self {
        match e {
            AuthorizationError::PermissionDenied(e) => {
                MononokeError::AuthorizationError(e.to_string())
            }
            AuthorizationError::Error(e) => MononokeError::InternalError(InternalError::from(e)),
        }
    }
}

impl From<BlameError> for MononokeError {
    fn from(e: BlameError) -> Self {
        use BlameError::*;
        match e {
            NoSuchPath(_) | IsDirectory(_) | Rejected(_) => {
                MononokeError::InvalidRequest(e.to_string())
            }
            DeriveError(e) => MononokeError::from(e),
            _ => MononokeError::from(anyhow::Error::from(e)),
        }
    }
}

impl From<MononokeError> for edenapi_types::ServerError {
    fn from(e: MononokeError) -> Self {
        edenapi_types::ServerError::from(&e)
    }
}

impl From<&MononokeError> for edenapi_types::ServerError {
    fn from(e: &MononokeError) -> Self {
        let message = format!("{:?}", e);
        let code = match e {
            MononokeError::InternalError(e)
                if e.0.is::<segmented_changelog::MismatchedHeadsError>() =>
            {
                1
            }
            _ => 0,
        };
        Self::new(message, code)
    }
}

impl From<MononokeError> for MegarepoError {
    fn from(e: MononokeError) -> Self {
        MegarepoError::internal(e)
    }
}
