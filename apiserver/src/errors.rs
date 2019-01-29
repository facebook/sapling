// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use actix::MailboxError;
use actix_web::error::ResponseError;
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use failure::{Error, Fail};
use futures::Canceled;

use api::errors::ErrorKind as ApiError;
use apiserver_thrift::types::{MononokeAPIException, MononokeAPIExceptionKind};
use blobrepo::ErrorKind as BlobRepoError;
use reachabilityindex::errors::ErrorKind as ReachabilityIndexError;

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum ErrorResponse {
    APIErrorResponse(APIErrorResponse),
    LFSErrorResponse(LFSErrorResponse),
}

#[derive(Serialize, Debug)]
struct APIErrorResponse {
    message: String,
    causes: Vec<String>,
}

#[derive(Serialize, Debug)]
struct LFSErrorResponse {
    message: String,
}

#[derive(Debug)]
pub enum ErrorKind {
    NotFound(String, Option<Error>),
    InvalidInput(String, Option<Error>),
    InternalError(Error),
    LFSNotFound(String),
}

impl ErrorKind {
    fn status_code(&self) -> StatusCode {
        use errors::ErrorKind::*;

        match self {
            NotFound(..) => StatusCode::NOT_FOUND,
            InvalidInput(..) => StatusCode::BAD_REQUEST,
            InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LFSNotFound(_) => StatusCode::NOT_FOUND,
        }
    }

    #[allow(deprecated)] // self.causes()
    fn into_error_response(&self) -> ErrorResponse {
        use errors::ErrorKind::*;

        match &self {
            NotFound(..) | InvalidInput(..) | InternalError(_) => {
                ErrorResponse::APIErrorResponse(APIErrorResponse {
                    message: self.to_string(),
                    causes: self
                        .causes()
                        .skip(1)
                        .map(|cause| cause.to_string())
                        .collect(),
                })
            }
            LFSNotFound(_) => ErrorResponse::LFSErrorResponse(LFSErrorResponse {
                message: self.to_string(),
            }),
        }
    }
}

impl Fail for ErrorKind {
    fn cause(&self) -> Option<&Fail> {
        use errors::ErrorKind::*;

        match self {
            NotFound(_, cause) | InvalidInput(_, cause) => cause.as_ref().map(|e| e.as_fail()),
            InternalError(err) => Some(err.as_fail()),
            LFSNotFound(_) => None,
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
            LFSNotFound(_0) => write!(f, "{} is not found on LFS request", _0),
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
                ErrorKind::InternalError(err) => err_downcast_ref! {
                    err,
                    err: ErrorKind => err,
                }
                .unwrap_or(self),
                _ => self,
            }
        };

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
        };
        ret.unwrap_or_else(|e| ErrorKind::InternalError(e))
    }
}

impl From<Canceled> for ErrorKind {
    fn from(e: Canceled) -> ErrorKind {
        let error = Error::from_boxed_compat(Box::new(e));
        ErrorKind::InternalError(error)
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
            e @ GenerationFetchFailed(_)
            | e @ ParentsFetchFailed(_)
            | e @ UknownSkiplistThriftEncoding => ErrorKind::InternalError(e.into()),
        }
    }
}

impl From<ErrorKind> for MononokeAPIException {
    fn from(e: ErrorKind) -> MononokeAPIException {
        use errors::ErrorKind::*;

        match e {
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
            e @ LFSNotFound(_) => MononokeAPIException {
                kind: MononokeAPIExceptionKind::NotFound,
                reason: e.to_string(),
            },
        }
    }
}
