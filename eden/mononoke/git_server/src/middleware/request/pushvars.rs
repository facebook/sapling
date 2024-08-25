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
const METAGIT_PUSHVAR_PREFIX: &str = "x-metagit";

#[derive(Clone)]
pub struct PushvarsParsingMiddleware {}

#[async_trait::async_trait]
impl Middleware for PushvarsParsingMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let headers = HeaderMap::try_borrow_from(state)?;
        let pushvars = headers
            .iter()
            .filter_map(|(name, value)| {
                let name = name.as_str().to_lowercase();
                if name.starts_with(METAGIT_PUSHVAR_PREFIX) || name.starts_with(PUSHVAR_PREFIX) {
                    return Some((name, Bytes::copy_from_slice(value.as_bytes())));
                } else {
                    None
                }
            })
            .collect();
        state.put(Pushvars::new(pushvars));
        None
    }
}
