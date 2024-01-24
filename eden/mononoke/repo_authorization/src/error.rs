/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use bookmarks::BookmarkKey;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use permission_checker::MononokeIdentitySet;
use thiserror::Error;

use crate::context::AuthorizationContext;
use crate::context::RepoWriteOperation;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DeniedAction {
    FullRepoRead,
    FullRepoDraft,
    RepoMetadataRead,
    PathRead(ChangesetId, MPath),
    RepoWrite(RepoWriteOperation),
    PathWrite(NonRootMPath),
    BookmarkModification(BookmarkKey),
    OverrideGitMapping,
    GitImportOperation,
}

impl fmt::Display for DeniedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeniedAction::FullRepoRead => f.write_str("Full repo read access"),
            DeniedAction::FullRepoDraft => f.write_str("Full repo draft access"),
            DeniedAction::RepoMetadataRead => f.write_str("Repo metadata read access"),
            DeniedAction::PathRead(csid, path) => {
                if path.is_root() {
                    write!(f, "Repo read access for root of changeset {}", csid)
                } else {
                    write!(
                        f,
                        "Repo read access for path '{}' in changeset {}",
                        path, csid
                    )
                }
            }
            DeniedAction::RepoWrite(op) => write!(f, "Repo write access for {:?}", op),
            DeniedAction::PathWrite(path) => write!(f, "Repo write access to path '{}'", path),
            DeniedAction::BookmarkModification(bookmark) => {
                write!(f, "Modification of bookmark '{}'", bookmark)
            }
            DeniedAction::OverrideGitMapping => f.write_str("Overriding of Git mapping"),
            DeniedAction::GitImportOperation => {
                f.write_str("Access for Git-import related operations")
            }
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
