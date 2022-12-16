/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
#[error("remotenames key is not initalized in metalog")]
pub struct RemotenamesMetalogKeyError;

#[derive(Debug, Error)]
#[error("cannot initialize working copy: {0:?}")]
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
