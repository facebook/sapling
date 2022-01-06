/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Definition of errors used in this crate by the error_chain crate

use mononoke_types::RepositoryId;
use thiserror::Error;

/// Types of errors we can raise
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// The given bookmark does not exist in the repo
    #[error("bookmark not found: {0}")]
    BookmarkNotFound(String),
    /// The structure of metaconfig repo is invalid
    #[error("invalid file structure: {0}")]
    InvalidFileStructure(String),
    /// Config is invalid
    #[error("invalid config options: {0}")]
    InvalidConfig(String),
    /// Duplicated repo ids
    #[error("repoid {0} used more than once")]
    DuplicatedRepoId(RepositoryId),
    /// Missing path for hook
    #[error("missing path")]
    MissingPath(),
    /// Invalid pushvar
    #[error("invalid pushvar, should be KEY=VALUE: {0}")]
    InvalidPushvar(String),
}
