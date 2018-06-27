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

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "{} not found", _0)] NotFound(String),
    #[fail(display = "{} is invalid", _0)] InvalidInput(String),
    #[fail(display = "internal server error: {}", _0)] InternalError(Error),
}

impl ErrorKind {
    fn status_code(&self) -> StatusCode {
        use errors::ErrorKind::*;

        match self {
            NotFound(_) => StatusCode::NOT_FOUND,
            InvalidInput(_) => StatusCode::BAD_REQUEST,
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
            .unwrap_or_else(|e| ErrorKind::InternalError(e))
    }
}

impl From<MailboxError> for ErrorKind {
    fn from(e: MailboxError) -> ErrorKind {
        ErrorKind::InternalError(e.into())
    }
}
