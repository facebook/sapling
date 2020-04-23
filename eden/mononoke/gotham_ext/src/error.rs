/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter;

use anyhow::Error;
use gotham::{
    handler::{HandlerError, IntoHandlerError},
    helpers::http::response::create_response,
    state::{request_id, State},
};
use hyper::{Body, Response, StatusCode};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};

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

    /// Turn this error into a type corresponding to the return type
    /// of a Gotham handler, so that it may be directly returned from
    /// a handler function.
    pub fn into_handler_response(
        self,
        state: State,
    ) -> Result<(State, Response<Body>), (State, HandlerError)> {
        // Concatenate all chained errors into a single string.
        let message = iter::once(self.error.to_string())
            .chain(self.error.chain().skip(1).map(|c| c.to_string()))
            .join(": ");

        // Package the error message into a JSON response.
        let res = JsonError {
            message,
            request_id: request_id(&state).to_string(),
        };

        // Convert to JSON; should not fail but return a handler error if so.
        match serde_json::to_string(&res) {
            Ok(res) => {
                let res = create_response(&state, self.status_code, mime::APPLICATION_JSON, res);
                Ok((state, res))
            }
            Err(error) => Err((state, error.into_handler_error())),
        }
    }
}

/// JSON representation of an error to send to the client.
#[derive(Clone, Serialize, Debug, Deserialize)]
struct JsonError {
    message: String,
    request_id: String,
}
