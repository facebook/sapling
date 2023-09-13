/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Convert between Thrift and native Rust types.

use serde::Deserialize;
use serde::Serialize;
use thrift_types::edenfs;
use types::RepoPathBuf;

// edenfs::ScmFileStatus
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub enum FileStatus {
    #[serde(rename = "A")]
    Added,
    #[serde(rename = "M")]
    Modified,
    #[serde(rename = "R")]
    Removed,
    #[serde(rename = "I")]
    Ignored,
    // Ideally there is also an "Error" state (cannot download file).
}

// edenfs::CheckoutMode
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckoutMode {
    Normal,
    Force,
    DryRun,
}

// edenfs::ConflictType
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConflictType {
    /// We failed to update this particular path due to an error.
    Error,
    /// A locally modified file was deleted in the new Tree.
    ModifiedRemoved,
    /// An untracked local file exists in the new Tree.
    UntrackedAdded,
    /// The file was removed locally, but modified in the new Tree.
    RemovedModified,
    /// The file was removed locally, and also removed in the new Tree.
    MissingRemoved,
    /// A locally modified file was modified in the new Tree
    /// This may be contents modifications, or a file type change
    /// (directory to\nfile or vice-versa), or permissions changes.
    ModifiedModified,
    /// A directory was supposed to be removed or replaced with a file,
    /// but it contains untracked files preventing us from updating it.
    DirectoryNotEmpty,
}

// edenfs::CheckoutConflict
#[derive(Clone, Debug, Serialize)]
pub struct CheckoutConflict {
    pub path: RepoPathBuf,
    pub conflict_type: ConflictType,
    pub message: String,
}

// edenfs::EdenError
#[derive(Debug, Serialize)]
pub struct EdenError {
    pub message: String,
    pub error_code: Option<i32>,
    pub error_type: String,
}

impl From<edenfs::ScmFileStatus> for FileStatus {
    fn from(status: edenfs::ScmFileStatus) -> Self {
        match status {
            edenfs::ScmFileStatus::ADDED => FileStatus::Added,
            edenfs::ScmFileStatus::MODIFIED => FileStatus::Modified,
            edenfs::ScmFileStatus::REMOVED => FileStatus::Removed,
            edenfs::ScmFileStatus::IGNORED => FileStatus::Ignored,
            _ => panic!("unexpected ScmFileStatus: {}", status),
        }
    }
}

impl From<CheckoutMode> for edenfs::CheckoutMode {
    fn from(val: CheckoutMode) -> Self {
        match val {
            CheckoutMode::Normal => edenfs::CheckoutMode::NORMAL,
            CheckoutMode::Force => edenfs::CheckoutMode::FORCE,
            CheckoutMode::DryRun => edenfs::CheckoutMode::DRY_RUN,
        }
    }
}

impl From<edenfs::ConflictType> for ConflictType {
    fn from(conflict_type: edenfs::ConflictType) -> Self {
        match conflict_type {
            edenfs::ConflictType::ERROR => ConflictType::Error,
            edenfs::ConflictType::MODIFIED_REMOVED => ConflictType::ModifiedRemoved,
            edenfs::ConflictType::UNTRACKED_ADDED => ConflictType::UntrackedAdded,
            edenfs::ConflictType::REMOVED_MODIFIED => ConflictType::RemovedModified,
            edenfs::ConflictType::MISSING_REMOVED => ConflictType::MissingRemoved,
            edenfs::ConflictType::MODIFIED_MODIFIED => ConflictType::ModifiedModified,
            edenfs::ConflictType::DIRECTORY_NOT_EMPTY => ConflictType::DirectoryNotEmpty,
            _ => panic!("unexpected ConflictType: {}", conflict_type),
        }
    }
}

impl TryFrom<edenfs::CheckoutConflict> for CheckoutConflict {
    type Error = ();

    fn try_from(conflict: edenfs::CheckoutConflict) -> Result<Self, Self::Error> {
        let path = RepoPathBuf::from_utf8(conflict.path).map_err(|e| {
            tracing::warn!("conflict: ignore non-utf8 path {}", e);
        })?;
        let conflict_type = conflict.r#type.into();
        let message = conflict.message;
        Ok(CheckoutConflict {
            path,
            conflict_type,
            message,
        })
    }
}

impl std::fmt::Display for EdenError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "EdenError: {}", self.message)
    }
}

impl std::error::Error for EdenError {}

impl TryFrom<&(dyn std::error::Error + 'static)> for EdenError {
    type Error = ();

    fn try_from(value: &(dyn std::error::Error + 'static)) -> Result<Self, Self::Error> {
        if let Some(value) = value.downcast_ref::<edenfs::EdenError>() {
            let eden_error = Self {
                message: value.message.clone(),
                error_code: value.errorCode,
                error_type: value.errorType.to_string(),
            };
            Ok(eden_error)
        } else {
            Err(())
        }
    }
}
