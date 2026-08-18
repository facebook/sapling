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
    /// A tier manifest entry's repo_id disagrees with the RepoSpec its
    /// config_path points at
    #[error(
        "repo {repo_name}: manifest lists repo_id {manifest_repo_id} but its RepoSpec has repo_id {spec_repo_id}"
    )]
    RepoIdMismatch {
        /// Name of the repo as listed in the manifest entry
        repo_name: String,
        /// repo_id from the manifest entry
        manifest_repo_id: RepositoryId,
        /// repo_id from the repo's RepoSpec
        spec_repo_id: RepositoryId,
    },
    /// Missing path for hook
    #[error("missing path")]
    MissingPath(),
    /// Invalid pushvar
    #[error("invalid pushvar, should be KEY=VALUE: {0}")]
    InvalidPushvar(String),
}
