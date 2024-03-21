/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Convert between Thrift and native Rust types.

use serde::Serialize;
use thrift_types::edenfs;
pub use types::workingcopy_client::CheckoutConflict;
pub use types::workingcopy_client::CheckoutMode;
pub use types::workingcopy_client::ConflictType;
pub use types::workingcopy_client::FileStatus;
use types::RepoPathBuf;

// edenfs::EdenError
#[derive(Debug, Serialize)]
pub struct EdenError {
    pub message: String,
    pub error_code: Option<i32>,
    pub error_type: String,
}

/// Crate-local `From` to workaround Rust orphan rule.
pub trait LocalFrom<T> {
    fn local_from(v: T) -> Self;
}
pub trait LocalTryFrom<T>: Sized {
    type Error;
    fn local_try_from(v: T) -> Result<Self, Self::Error>;
}

impl LocalFrom<edenfs::ScmFileStatus> for FileStatus {
    fn local_from(status: edenfs::ScmFileStatus) -> Self {
        match status {
            edenfs::ScmFileStatus::ADDED => FileStatus::Added,
            edenfs::ScmFileStatus::MODIFIED => FileStatus::Modified,
            edenfs::ScmFileStatus::REMOVED => FileStatus::Removed,
            edenfs::ScmFileStatus::IGNORED => FileStatus::Ignored,
            _ => panic!("unexpected ScmFileStatus: {}", status),
        }
    }
}

impl LocalFrom<CheckoutMode> for edenfs::CheckoutMode {
    fn local_from(val: CheckoutMode) -> Self {
        match val {
            CheckoutMode::Normal => edenfs::CheckoutMode::NORMAL,
            CheckoutMode::Force => edenfs::CheckoutMode::FORCE,
            CheckoutMode::DryRun => edenfs::CheckoutMode::DRY_RUN,
        }
    }
}

impl LocalFrom<edenfs::ConflictType> for ConflictType {
    fn local_from(conflict_type: edenfs::ConflictType) -> Self {
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

impl LocalTryFrom<edenfs::CheckoutConflict> for CheckoutConflict {
    type Error = ();

    fn local_try_from(conflict: edenfs::CheckoutConflict) -> Result<Self, Self::Error> {
        let path = RepoPathBuf::from_utf8(conflict.path).map_err(|e| {
            tracing::warn!("conflict: ignore non-utf8 path {}", e);
        })?;
        let conflict_type = ConflictType::local_from(conflict.r#type);
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

impl LocalTryFrom<&(dyn std::error::Error + 'static)> for EdenError {
    type Error = ();

    fn local_try_from(value: &(dyn std::error::Error + 'static)) -> Result<Self, Self::Error> {
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
