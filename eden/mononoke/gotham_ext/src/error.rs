/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use gotham::state::State;
use hyper::Body;
use hyper::StatusCode;
use mime::Mime;
use rate_limiting::RateLimitReason;

pub trait ErrorFormatter {
    type Body: Into<Body>;

    fn format(&self, error: &Error, state: &State) -> Result<(Self::Body, Mime), Error>;
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
}

impl From<RateLimitReason> for HttpError {
    fn from(r: RateLimitReason) -> Self {
        Self::e429(r)
    }
}
