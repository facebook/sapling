/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use futures::Stream;
use gotham::state::State;
use gotham_ext::{
    error::HttpError,
    response::{StreamBody, TryIntoResponse},
};
use hyper::{Body, Response};
use mime::Mime;

use crate::errors::LfsServerContextErrorKind;
use crate::middleware::RequestContext;

impl From<LfsServerContextErrorKind> for HttpError {
    fn from(e: LfsServerContextErrorKind) -> HttpError {
        use LfsServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e400(e),
            PermissionCheckFailed(_) => HttpError::e500(e),
        }
    }
}

/// Wrapper around `gotham_ext::StreamBody` that will signal the
/// current `RequestContext`'s post-request callback upon completion.
pub struct LfsStreamBody<S>(StreamBody<S>);

impl<S> LfsStreamBody<S> {
    pub fn new(stream: S, content_length: u64, mime: Mime) -> Self {
        Self(StreamBody::new(stream, mime).content_length(content_length))
    }
}

impl<S> TryIntoResponse for LfsStreamBody<S>
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        match state.try_borrow_mut::<RequestContext>() {
            Some(ctx) => self.0.signal(ctx.delay_post_request()),
            None => self.0,
        }
        .try_into_response(state)
    }
}
