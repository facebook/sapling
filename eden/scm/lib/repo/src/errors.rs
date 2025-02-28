/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
#[error("repository {0} not found!")]
pub struct RepoNotFound(pub String);

#[derive(Debug, Error)]
#[error("sharedpath points to nonexistent directory {0}!")]
pub struct InvalidSharedPath(pub String);

#[derive(Debug, Error)]
#[error("remotenames key is not initialized in metalog")]
pub struct RemotenamesMetalogKeyError;

#[derive(Debug, Error)]
#[error("cannot initialize working copy")]
pub struct InvalidWorkingCopy(#[from] anyhow::Error);

#[derive(Error, Debug)]
pub enum InitError {
    #[error("repository `{0}` already exists")]
    ExistingRepoError(PathBuf),

    #[error("unable to create directory at `{0}`: `{1}`")]
    DirectoryCreationError(String, std::io::Error),

    #[error("unable to create file at `{0}`: `{1}`")]
    FileCreationError(PathBuf, std::io::Error),

    #[error("config loading error: `{0}`")]
    ConfigLoadingError(anyhow::Error),
}

impl From<configmodel::Error> for InitError {
    fn from(e: configmodel::Error) -> Self {
        Self::ConfigLoadingError(e.into())
    }
}
