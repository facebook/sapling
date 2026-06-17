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
    #[error(
        "Operation not permitted: identities [{identities}] do not have '{action}' access on ACL REPO:{acl}. To request access, visit https://www.internalfb.com/amp/ACL/REPO:{acl}"
    )]
    ForbiddenByAcl {
        identities: String,
        acl: String,
        action: &'static str,
    },
    #[error("Operation not permitted: no Hipster ACL registered for repo '{repo_name}'.")]
    ForbiddenNoAcl { repo_name: String },
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
            ForbiddenByAcl { .. } | ForbiddenNoAcl { .. } => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e404(e),
            NotAuthenticated => HttpError::e403(e),
            RepositoryNotLoaded(_) => HttpError::e503(e),
            RepoSetupError { .. } => HttpError::e500(e),
        }
    }
}
