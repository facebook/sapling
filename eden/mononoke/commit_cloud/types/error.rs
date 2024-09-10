/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow;
use thiserror::Error;
#[derive(Debug, Error, Clone, PartialEq, Eq, Hash)]
pub enum CommitCloudUserError {
    #[error("Workspace {0} does not exist for repo {1}")]
    NonexistantWorkspace(String, String),
}

#[derive(Debug, Error)]
pub enum CommitCloudInternalError {
    #[error(transparent)]
    Error(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum CommitCloudError {
    UserError(CommitCloudUserError),
    InternalError(CommitCloudInternalError),
}

impl std::fmt::Display for CommitCloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitCloudError::UserError(e) => write!(f, "{}", e),
            CommitCloudError::InternalError(e) => write!(f, "{}", e),
        }
    }
}

impl From<CommitCloudUserError> for CommitCloudError {
    fn from(error: CommitCloudUserError) -> Self {
        CommitCloudError::UserError(error)
    }
}

impl From<CommitCloudInternalError> for CommitCloudError {
    fn from(error: CommitCloudInternalError) -> Self {
        CommitCloudError::InternalError(error)
    }
}
