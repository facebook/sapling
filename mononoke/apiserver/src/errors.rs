/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use actix::MailboxError;
use actix_web::error::ResponseError;
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use anyhow::{anyhow, Error};
use failure_ext::{err_downcast, err_downcast_ref};
use futures::Canceled;
use serde_derive::Serialize;
use std::error::Error as _;
use thiserror::Error;

use apiserver_thrift::types::{MononokeAPIException, MononokeAPIExceptionKind};
use blobrepo::ErrorKind as BlobRepoError;
use mercurial_types::blobs::ErrorKind as MercurialBlobError;
use mononoke_api::legacy::ErrorKind as ApiError;
use reachabilityindex::errors::ErrorKind as ReachabilityIndexError;

#[derive(Serialize, Debug)]
struct ErrorResponse {
    message: String,
    causes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("{0} is not found")]
    NotFound(String, #[source] Option<Error>),
    #[error("Invalid input: {0}")]
    InvalidInput(String, #[source] Option<Error>),
    #[error("internal server error: {0}")]
    InternalError(#[source] Error),
    #[error("{0} is not a directory")]
    NotADirectory(String),
    #[error("{0} is not a valid bookmark")]
    BookmarkNotFound(String),
}

impl ErrorKind {
    fn status_code(&self) -> StatusCode {
        use crate::errors::ErrorKind::*;

        match self {
            NotFound(..) => StatusCode::NOT_FOUND,
            InvalidInput(..) => StatusCode::BAD_REQUEST,
            InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            NotADirectory(_) => StatusCode::BAD_REQUEST,
            BookmarkNotFound(_) => StatusCode::BAD_REQUEST,
        }
    }

    pub fn is_server_error(&self) -> bool {
        match self {
            ErrorKind::InternalError(_) => true,
            _ => false,
        }
    }

    fn into_error_response(&self) -> ErrorResponse {
        let mut causes = Vec::new();
        let mut next = self.source();
        while let Some(cause) = next {
            causes.push(cause.to_string());
            next = cause.source();
        }

        ErrorResponse {
            message: self.to_string(),
            causes,
        }
    }

    // Since all non-ErrorKind error including `Context<ErrorKind>` is wrapped in `InternalError`
    // automatically at `From<Error>::from`, we need to downcast the `Context` retrieve the
    // `ErrorKind` in the `Context`.
    fn unwrap_errorkind(&self) -> &Self {
        match self {
            ErrorKind::InternalError(err) => err_downcast_ref! {
                err,
                err: ErrorKind => err,
            }
            .unwrap_or(self),
            _ => self,
        }
    }
}

impl ResponseError for ErrorKind {
    fn error_response(&self) -> HttpResponse {
        let err = self.unwrap_errorkind();
        HttpResponse::build(err.status_code()).json(err.into_error_response())
    }
}

impl From<Error> for ErrorKind {
    fn from(err: Error) -> ErrorKind {
        let ret = err_downcast! {
            err,
            e: BlobRepoError => ErrorKind::from(e),
            e: ApiError => ErrorKind::from(e),
            e: ReachabilityIndexError => ErrorKind::from(e),
            e: MercurialBlobError => ErrorKind::from(e),
        };
        ret.unwrap_or_else(|e| ErrorKind::InternalError(e))
    }
}

impl From<Canceled> for ErrorKind {
    fn from(e: Canceled) -> ErrorKind {
        ErrorKind::InternalError(anyhow!(e))
    }
}

impl From<MailboxError> for ErrorKind {
    fn from(e: MailboxError) -> ErrorKind {
        ErrorKind::InternalError(anyhow!(e))
    }
}

impl From<ApiError> for ErrorKind {
    fn from(e: ApiError) -> ErrorKind {
        use self::ApiError::*;

        match e {
            NotFound(t) => ErrorKind::NotFound(t, None),
        }
    }
}

impl From<BlobRepoError> for ErrorKind {
    fn from(e: BlobRepoError) -> ErrorKind {
        use self::BlobRepoError::*;

        // TODO: changes this match to P59854201 when NLL is stabilized
        match e {
            ChangesetMissing(id) => {
                ErrorKind::NotFound(id.to_string(), Some(ChangesetMissing(id).into()))
            }
            ManifestMissing(id) => {
                ErrorKind::NotFound(id.to_string(), Some(ManifestMissing(id).into()))
            }
            _ => ErrorKind::InternalError(e.into()),
        }
    }
}

impl From<ReachabilityIndexError> for ErrorKind {
    fn from(e: ReachabilityIndexError) -> ErrorKind {
        use self::ReachabilityIndexError::*;

        match e {
            NodeNotFound(s) => ErrorKind::NotFound(s.clone(), Some(NodeNotFound(s).into())),
            CheckExistenceFailed(s, t) => {
                ErrorKind::NotFound(s.clone(), Some(CheckExistenceFailed(s, t).into()))
            }
            e @ GenerationFetchFailed(_)
            | e @ ParentsFetchFailed(_)
            | e @ UknownSkiplistThriftEncoding => ErrorKind::InternalError(e.into()),
        }
    }
}

impl From<MercurialBlobError> for ErrorKind {
    fn from(e: MercurialBlobError) -> Self {
        use self::MercurialBlobError::*;
        match e {
            HgContentMissing(id, _t) => {
                ErrorKind::NotFound(id.to_string(), Some(HgContentMissing(id, _t).into()))
            }
            _ => ErrorKind::InternalError(e.into()),
        }
    }
}

impl From<ErrorKind> for MononokeAPIException {
    fn from(e: ErrorKind) -> MononokeAPIException {
        use crate::errors::ErrorKind::*;

        match e.unwrap_errorkind() {
            e @ NotFound(..) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::NotFound,
                reason: e.to_string(),
            },
            e @ InvalidInput(..) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::InvalidInput,
                reason: e.to_string(),
            },
            e @ InternalError(_) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::InternalError,
                reason: e.to_string(),
            },
            e @ NotADirectory(_) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::InvalidInput,
                reason: e.to_string(),
            },
            e @ BookmarkNotFound(_) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::BookmarkNotFound,
                reason: e.to_string(),
            },
        }
    }
}
