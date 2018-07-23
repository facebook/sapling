// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix::MailboxError;
use actix_web::HttpResponse;
use actix_web::error::ResponseError;
use actix_web::http::StatusCode;
use failure::{Context, Error};

use api::errors::ErrorKind as ApiError;
use blobrepo::ErrorKind as BlobRepoError;
use reachabilityindex::errors::ErrorKind as ReachabilityIndexError;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "{} not found", _0)] NotFound(String),
    #[fail(display = "{} is invalid", _0)] InvalidInput(String),
    #[fail(display = "could not fetch node generation")] GenerationFetchFailed,
    #[fail(display = "failed to fetch parent nodes")] ParentsFetchFailed,
    #[fail(display = "internal server error: {}", _0)] InternalError(Error),
}

impl ErrorKind {
    fn status_code(&self) -> StatusCode {
        use errors::ErrorKind::*;

        match self {
            NotFound(_) => StatusCode::NOT_FOUND,
            InvalidInput(_) => StatusCode::BAD_REQUEST,
            GenerationFetchFailed => StatusCode::BAD_REQUEST,
            ParentsFetchFailed => StatusCode::BAD_REQUEST,
            InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl ResponseError for ErrorKind {
    // Since all non-ErrorKind error including `Context<ErrorKind>` is wrapped in `InternalError`
    // automatically at `From<Error>::from`, we need to downcast the `Context` retrieve the
    // `ErrorKind` in the `Context`.
    fn error_response(&self) -> HttpResponse {
        let err = {
            match self {
                ErrorKind::InternalError(err) => err.downcast_ref::<Context<ErrorKind>>()
                    .map(|e| e.get_context())
                    .unwrap_or_else(|| self),
                _ => self,
            }
        };

        HttpResponse::build(err.status_code()).body(err.to_string())
    }
}

impl From<Error> for ErrorKind {
    fn from(e: Error) -> ErrorKind {
        e.downcast::<ErrorKind>()
            .or_else(|err| err.downcast::<BlobRepoError>().map(|e| e.into()))
            .or_else(|err| err.downcast::<ApiError>().map(|e| e.into()))
            .or_else(|err| err.downcast::<ReachabilityIndexError>().map(|e| e.into()))
            .unwrap_or_else(|e| ErrorKind::InternalError(e))
    }
}

impl From<MailboxError> for ErrorKind {
    fn from(e: MailboxError) -> ErrorKind {
        ErrorKind::InternalError(e.into())
    }
}

impl From<ApiError> for ErrorKind {
    fn from(e: ApiError) -> ErrorKind {
        use self::ApiError::*;

        match e {
            NotFound(t) => ErrorKind::NotFound(t),
            InvalidInput(t) => ErrorKind::InvalidInput(t),
        }
    }
}

impl From<BlobRepoError> for ErrorKind {
    fn from(e: BlobRepoError) -> ErrorKind {
        use self::BlobRepoError::*;

        match e {
            ChangesetMissing(cs) => ErrorKind::NotFound(cs.to_string()),
            e => ErrorKind::InternalError(e.into()),
        }
    }
}

impl From<ReachabilityIndexError> for ErrorKind {
    fn from(e: ReachabilityIndexError) -> ErrorKind {
        match e {
            ReachabilityIndexError::NodeNotFound(s) => ErrorKind::NotFound(s),
            ReachabilityIndexError::GenerationFetchFailed(_) => ErrorKind::GenerationFetchFailed,
            ReachabilityIndexError::ParentsFetchFailed(_) => ErrorKind::ParentsFetchFailed,
        }
    }
}
