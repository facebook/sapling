/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;
use std::time::Instant;

use gotham::state::State;
use gotham_derive::StateData;
use hyper::Body;
use hyper::Response;

use super::Middleware;

#[derive(StateData, Debug, Copy, Clone)]
pub struct RequestStartTime(pub Instant);

#[derive(StateData, Debug, Copy, Clone)]
pub struct HeadersDuration(pub Duration);

#[derive(Clone)]
pub struct TimerMiddleware;

impl TimerMiddleware {
    pub fn new() -> Self {
        TimerMiddleware
    }
}

#[async_trait::async_trait]
impl Middleware for TimerMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        state.put(RequestStartTime(Instant::now()));
        None
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        if let Some(RequestStartTime(start)) = state.try_borrow() {
            let headers_duration = start.elapsed();
            state.put(HeadersDuration(headers_duration));
        }
    }
}
