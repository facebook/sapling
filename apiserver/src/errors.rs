// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use actix::MailboxError;
use actix_web::HttpResponse;
use actix_web::error::ResponseError;
use actix_web::http::StatusCode;
use failure::{Context, Error, Fail};

use api::errors::ErrorKind as ApiError;
use blobrepo::ErrorKind as BlobRepoError;
use reachabilityindex::errors::ErrorKind as ReachabilityIndexError;

#[derive(Serialize, Debug)]
struct ErrorResponse {
    message: String,
    causes: Vec<String>,
}

#[derive(Debug)]
pub enum ErrorKind {
    NotFound(String, Option<Error>),
    InvalidInput(String, Option<Error>),
    InternalError(Error),
}

impl ErrorKind {
    fn status_code(&self) -> StatusCode {
        use errors::ErrorKind::*;

        match self {
            NotFound(..) => StatusCode::NOT_FOUND,
            InvalidInput(..) => StatusCode::BAD_REQUEST,
            InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn into_error_response(&self) -> ErrorResponse {
        ErrorResponse {
            message: self.to_string(),
            causes: self.causes()
                .skip(1)
                .map(|cause| cause.to_string())
                .collect(),
        }
    }
}

impl Fail for ErrorKind {
    fn cause(&self) -> Option<&Fail> {
        use errors::ErrorKind::*;

        match self {
            NotFound(_, cause) | InvalidInput(_, cause) => cause.as_ref().map(|e| e.cause()),
            InternalError(err) => Some(err.cause()),
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use errors::ErrorKind::*;

        match self {
            NotFound(_0, _) => write!(f, "{} is not found", _0),
            InvalidInput(_0, _) => write!(f, "{} is invalid", _0),
            InternalError(_0) => write!(f, "internal server error: {}", _0),
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

        HttpResponse::build(err.status_code()).json(err.into_error_response())
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
            NotFound(t) => ErrorKind::NotFound(t, None),
            InvalidInput(t) => ErrorKind::InvalidInput(t, None),
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
            HgContentMissing(id, _t) => {
                ErrorKind::NotFound(id.to_string(), Some(HgContentMissing(id, _t).into()))
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
            e @ GenerationFetchFailed(_) | e @ ParentsFetchFailed(_) => {
                ErrorKind::InternalError(e.into())
            }
        }
    }
}
