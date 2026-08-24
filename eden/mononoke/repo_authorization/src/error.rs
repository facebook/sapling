/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use bookmarks::BookmarkKey;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;
use permission_checker::MononokeIdentitySet;
use permission_checker::PermissionDenial;
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
    CommitCloudOperation(String, String),
    CreateRepo,
    MirrorUpload,
}

impl fmt::Display for DeniedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeniedAction::FullRepoRead => f.write_str("Full repo read access"),
            DeniedAction::FullRepoDraft => f.write_str("Full repo draft access"),
            DeniedAction::RepoMetadataRead => f.write_str("Repo metadata read access"),
            DeniedAction::PathRead(csid, path) => {
                if path.is_root() {
                    write!(f, "Repo read access for root of changeset {csid}")
                } else {
                    write!(f, "Repo read access for path '{path}' in changeset {csid}")
                }
            }
            DeniedAction::RepoWrite(op) => write!(f, "Repo write access for {op:?}"),
            DeniedAction::PathWrite(path) => write!(f, "Repo write access to path '{path}'"),
            DeniedAction::BookmarkModification(bookmark) => {
                write!(f, "Modification of bookmark '{bookmark}'")
            }
            DeniedAction::OverrideGitMapping => f.write_str("Overriding of Git mapping"),
            DeniedAction::GitImportOperation => {
                f.write_str("Access for Git-import related operations")
            }
            DeniedAction::CommitCloudOperation(action, workspace_acl) => f.write_str(
                format!("Access for Commit Cloud operation {action} for workspace {workspace_acl}")
                    .as_str(),
            ),
            DeniedAction::CreateRepo => f.write_str("Repository creation"),
            DeniedAction::MirrorUpload => f.write_str("Mirror upload"),
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error(
    "{denied_action} in repo '{denied_repo_name}' is not permitted with {context:?} for [{}]{}",
    .identities.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "),
    describe_denial(.denial)
)]
pub struct PermissionDenied {
    pub(crate) denied_action: DeniedAction,
    pub(crate) denied_repo_name: String,
    pub(crate) context: AuthorizationContext,
    pub(crate) identities: MononokeIdentitySet,
    /// What the access checker said, when the denial came from an ACL check
    /// that reported a reason.
    pub(crate) denial: Option<PermissionDenial>,
}

/// Only checkers that report a reason add one; otherwise the message is
/// unchanged. Without this the user is left to guess whether they are missing a
/// grant or were rejected by a policy.
fn describe_denial(denial: &Option<PermissionDenial>) -> String {
    match denial {
        Some(denial) if denial.is_informative() => format!(": {denial}"),
        _ => String::new(),
    }
}

#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error(transparent)]
    PermissionDenied(#[from] Box<PermissionDenied>),

    #[error(transparent)]
    Error(#[from] Error),
}
