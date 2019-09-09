// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::{handler::HandlerError, state::State};
use hyper::{Body, Response};
use lazy_static::lazy_static;
use mime;
use std::result::Result;
use std::str::FromStr;

pub type HandlerResponse = Result<(State, Response<Body>), (State, HandlerError)>;

lazy_static! {
    static ref GIT_LFS_MIME: mime::Mime =
        mime::Mime::from_str("application/vnd.git-lfs+json").unwrap();
}

#[macro_export]
macro_rules! bail_http {
    ($state: expr, $status: expr, $expr:expr $(,)?) => {
        match $expr {
            Ok(val) => val,
            Err(err) => {
                let res = ResponseError {
                    message: err.to_string(),
                    documentation_url: None,
                    request_id: Some(::gotham::state::request_id(&$state).to_string()),
                };

                // NOTE: If we can't serialize an error response, then there really isn't much we
                // can do.
                let res = match ::serde_json::to_string(&res) {
                    Ok(res) => res,
                    Err(err) => return Err(($state, err.into_handler_error())),
                };

                let res = ::gotham::helpers::http::response::create_response(
                    &$state,
                    $status,
                    git_lfs_mime(),
                    res,
                );
                return Ok(($state, res));
            }
        }
    };
}

#[macro_export]
macro_rules! bail_http_400 {
    ($state: expr, $expr:expr $(,)?) => {
        crate::bail_http!($state, ::hyper::StatusCode::BAD_REQUEST, $expr);
    };
}

#[macro_export]
macro_rules! bail_http_404 {
    ($state: expr, $expr:expr $(,)?) => {
        crate::bail_http!($state, ::hyper::StatusCode::NOT_FOUND, $expr);
    };
}

#[macro_export]
macro_rules! bail_http_500 {
    ($state: expr, $expr:expr $(,)?) => {
        crate::bail_http!($state, ::hyper::StatusCode::INTERNAL_SERVER_ERROR, $expr);
    };
}

pub fn git_lfs_mime() -> mime::Mime {
    GIT_LFS_MIME.clone()
}
