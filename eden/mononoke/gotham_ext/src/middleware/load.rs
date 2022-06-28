/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use gotham::state::State;
use gotham_derive::StateData;
use hyper::Body;
use hyper::Response;

use super::Middleware;
use super::PostResponseCallbacks;

#[derive(StateData, Debug, Copy, Clone)]
pub struct RequestLoad(pub i64);

impl Display for RequestLoad {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "l")?;
        fmt::Display::fmt(&self.0, fmt)
    }
}

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

#[async_trait::async_trait]
impl Middleware for LoadMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let old_request_count = self.requests.fetch_add(1, Ordering::Relaxed);
        state.put(RequestLoad(old_request_count + 1));
        None
    }

    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(request_load) = state.try_take::<RequestLoad>() {
            let headers = response.headers_mut();
            headers.insert("X-Load", request_load.0.into());
        }

        if let Some(callbacks) = state.try_borrow_mut::<PostResponseCallbacks>() {
            callbacks.add({
                let requests = self.requests.clone();
                move |_| {
                    requests.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }
    }
}
