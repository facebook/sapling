/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use thiserror::Error;

/// Error types that can occur during diff operations.
/// This allows proper categorization of user input errors vs internal system errors.
#[derive(Debug, Error)]
pub enum DiffError {
    /// User input validation errors - should be returned as RequestError
    #[error("{0}")]
    InvalidInput(String),
    /// Internal system errors - should be returned as InternalError  
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl DiffError {
    /// Create an error for when all diff inputs are empty
    pub fn empty_inputs() -> Self {
        DiffError::InvalidInput("All inputs to the headerless diff were empty".to_string())
    }

    /// Create an error for when content is not found
    pub fn content_not_found(content_id: ContentId) -> Self {
        DiffError::InvalidInput(format!("Content not found: {}", content_id))
    }

    /// Create an error for when a changeset is not found
    pub fn changeset_not_found(changeset_id: ChangesetId) -> Self {
        DiffError::InvalidInput(format!("changeset not found: {}", changeset_id))
    }

    /// Create an error for invalid path
    pub fn invalid_path(path: &str, error: impl std::fmt::Display) -> Self {
        DiffError::InvalidInput(format!("invalid path '{}': {}", path, error))
    }

    /// Create an internal error by wrapping another error
    pub fn internal(err: impl Into<anyhow::Error>) -> Self {
        DiffError::Internal(err.into())
    }

    /// Create an error for when string input exceeds size limit
    pub fn string_input_too_large(size: usize, max_size: usize) -> Self {
        DiffError::InvalidInput(format!(
            "String input size {} exceeds maximum allowed size of {} bytes",
            size, max_size
        ))
    }
}
