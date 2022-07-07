/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use bookmarks::BookmarkName;
use mononoke_types::MPath;
use permission_checker::MononokeIdentitySet;
use thiserror::Error;

use crate::context::AuthorizationContext;
use crate::context::RepoWriteOperation;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DeniedAction {
    FullRepoRead,
    RepoWrite(RepoWriteOperation),
    PathWrite(MPath),
    BookmarkModification(BookmarkName),
    OverrideGitMapping,
}

impl fmt::Display for DeniedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeniedAction::FullRepoRead => f.write_str("Full repo read access"),
            DeniedAction::RepoWrite(op) => write!(f, "Repo write access for {:?}", op),
            DeniedAction::PathWrite(path) => write!(f, "Repo write access to path '{}'", path),
            DeniedAction::BookmarkModification(bookmark) => {
                write!(f, "Modification of bookmark '{}'", bookmark)
            }
            DeniedAction::OverrideGitMapping => f.write_str("Overriding of Git mapping"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PermissionDenied {
    pub(crate) denied_action: DeniedAction,
    pub(crate) context: AuthorizationContext,
    pub(crate) identities: MononokeIdentitySet,
}

impl fmt::Display for PermissionDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not permitted with {:?} for [",
            self.denied_action, self.context
        )?;
        let mut delim = "";
        for id in self.identities.iter() {
            write!(f, "{}{}", delim, id)?;
            delim = ", ";
        }
        f.write_str("]")
    }
}

impl std::error::Error for PermissionDenied {}

#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error(transparent)]
    PermissionDenied(#[from] PermissionDenied),

    #[error(transparent)]
    Error(#[from] Error),
}
