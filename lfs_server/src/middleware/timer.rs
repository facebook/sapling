/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use gotham::state::State;
use hyper::{Body, Response};

use super::{Middleware, RequestContext};

#[derive(Clone)]
pub struct TimerMiddleware {}

impl TimerMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

impl Middleware for TimerMiddleware {
    fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        if let Some(ctx) = state.try_borrow_mut::<RequestContext>() {
            ctx.headers_ready();
        }
    }
}
