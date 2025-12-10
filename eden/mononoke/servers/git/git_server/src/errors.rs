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
}

impl From<GitServerContextErrorKind> for HttpError {
    fn from(e: GitServerContextErrorKind) -> HttpError {
        use GitServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e404(e),
            NotAuthenticated => HttpError::e403(e),
        }
    }
}
