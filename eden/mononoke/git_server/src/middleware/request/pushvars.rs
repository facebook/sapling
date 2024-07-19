/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::middleware::Middleware;
use http::HeaderMap;
use http::Response;
use hyper::Body;

use crate::model::Pushvars;

const PUSHVAR_PREFIX: &str = "x-git-";

#[derive(Clone)]
pub struct PushvarsParsingMiddleware {}

#[async_trait::async_trait]
impl Middleware for PushvarsParsingMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let headers = HeaderMap::try_borrow_from(state)?;
        let pushvars = headers
            .iter()
            .filter_map(|(name, value)| {
                name.as_str()
                    .to_lowercase()
                    .starts_with(PUSHVAR_PREFIX)
                    .then_some((name.to_string(), Bytes::copy_from_slice(value.as_bytes())))
            })
            .collect();
        state.put(Pushvars::new(pushvars));
        None
    }
}
