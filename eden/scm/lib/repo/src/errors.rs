/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Error)]
#[error("repository {0} not found!")]
pub struct RepoNotFound(pub String);

#[derive(Debug, Error)]
#[error(".hg/sharedpath points to nonexistent directory {0}!")]
pub struct InvalidSharedPath(pub String);

#[derive(Error, Debug)]
pub enum InitError {
    #[error("repository `{0}` already exists")]
    ExistingRepoError(String),

    #[error("unable to create directory at `{0}`: `{1}`")]
    DirectoryCreationError(String, String),

    #[error("unable to create file at `{0}`: `{1}`")]
    FileCreationError(String, String),

    #[error("config loading error: `{0}`")]
    ConfigLoadingError(String),
}
