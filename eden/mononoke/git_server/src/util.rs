/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use gotham_ext::response::EmptyBody;
use gotham_ext::response::TryIntoResponse;
use http::HeaderMap;
use http::Response;
use hyper::Body;

pub async fn get_body(state: &mut State) -> Result<Bytes, HttpError> {
    Body::take_from(state)
        .try_concat_body(&HeaderMap::new())
        .map_err(HttpError::e500)?
        .await
        .map_err(HttpError::e500)
}

pub fn empty_body(state: &mut State) -> Result<Response<Body>, HttpError> {
    EmptyBody::new()
        .try_into_response(state)
        .map_err(HttpError::e500)
}
