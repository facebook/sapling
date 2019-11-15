/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Definition of errors used in this crate by the error_chain crate

pub use failure_ext::{Error, Result};
use mononoke_types::RepositoryId;
use thiserror::Error;

/// Types of errors we can raise
#[derive(Debug, Error)]
pub enum ErrorKind {
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
    /// Too many bypass options for a hook
    #[error("Only one bypass option is allowed. Hook: {0}")]
    TooManyBypassOptions(String),
}
