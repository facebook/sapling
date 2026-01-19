/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham_ext::error::HttpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitServerContextErrorKind {
    #[error("Operation not permitted")]
    Forbidden,
    #[error("Client not authenticated")]
    NotAuthenticated,
    #[error("Repository does not exist: {0}")]
    RepositoryDoesNotExist(String),
    #[error("Repository not available on this server: {0}")]
    RepositoryNotLoaded(String),
    #[error("Failed to setup repository '{repo_name}': {error}")]
    RepoSetupError { repo_name: String, error: String },
}

impl From<GitServerContextErrorKind> for HttpError {
    fn from(e: GitServerContextErrorKind) -> HttpError {
        use GitServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e404(e),
            NotAuthenticated => HttpError::e403(e),
            RepositoryNotLoaded(_) => HttpError::e503(e),
            RepoSetupError { .. } => HttpError::e500(e),
        }
    }
}
