/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::Debug;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

use gotham::state::State;
use gotham_derive::StateData;
use hyper::{Body, Response};

use super::{Middleware, RequestContext};

#[derive(StateData, Debug)]
pub struct RequestLoad(pub i64);

#[derive(Clone)]
pub struct LoadMiddleware {
    // NOTE: This should always be >0 but considering this is for monitoring right now and
    // therefore isn't super sensitive, let's be a bit conservative and try to do something
    // reasonable on underflow by using an AtomicI64.
    requests: Arc<AtomicI64>,
}

impl LoadMiddleware {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(AtomicI64::new(0)),
        }
    }
}

impl Middleware for LoadMiddleware {
    fn inbound(&self, state: &mut State) {
        let requests = self.requests.fetch_add(1, Ordering::Relaxed);
        state.put(RequestLoad(requests));
    }

    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(request_load) = state.try_take::<RequestLoad>() {
            let headers = response.headers_mut();
            headers.insert("X-Load", request_load.0.into());
        }

        if let Some(ctx) = state.try_borrow_mut::<RequestContext>() {
            ctx.add_post_request({
                let requests = self.requests.clone();
                move |_, _| {
                    requests.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }
    }
}
