/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use gotham::{handler::HandlerError, helpers::http::response::create_response, state::State};
use hyper::{Body, Response, StatusCode};
use load_limiter::ThrottleReason;
use mime::Mime;

pub trait ErrorFormatter {
    type Body: Into<Body>;

    // TODO: Don't take &mut State here, once we've hoisted the error reporting into gotham_ext.
    fn format(&self, error: &Error, state: &mut State) -> Result<(Self::Body, Mime), Error>;
}

/// Wrapper around an anyhow::Error to indicate which
/// HTTP status code should be returned to the client.
pub struct HttpError {
    pub error: Error,
    pub status_code: StatusCode,
}

impl HttpError {
    pub fn e400<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::BAD_REQUEST,
        }
    }

    pub fn e403<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::FORBIDDEN,
        }
    }

    pub fn e404<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::NOT_FOUND,
        }
    }

    pub fn e410<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::GONE,
        }
    }

    pub fn e429<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::TOO_MANY_REQUESTS,
        }
    }

    pub fn e500<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn e503<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Turn this error into a type corresponding to the return type
    /// of a Gotham handler, so that it may be directly returned from
    /// a handler function.
    pub fn into_handler_response<F: ErrorFormatter>(
        self,
        mut state: State,
        formatter: &F,
    ) -> Result<(State, Response<Body>), (State, HandlerError)> {
        match formatter.format(&self.error, &mut state) {
            Ok((body, mime)) => {
                let res = create_response(&state, self.status_code, mime, body);
                Ok((state, res))
            }
            Err(error) => Err((state, error.into())),
        }
    }
}

impl From<ThrottleReason> for HttpError {
    fn from(r: ThrottleReason) -> Self {
        Self::e429(r)
    }
}
