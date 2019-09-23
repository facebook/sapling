// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::State;

use super::{Callback, Middleware, RequestContext};

#[derive(Clone)]
pub struct TimerMiddleware {}

impl TimerMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

impl Middleware for TimerMiddleware {
    fn handle(&self, _state: &mut State) -> Callback {
        // TODO: Rework the Callback stuff...
        Box::new(move |state, _response| {
            if let Some(ctx) = state.try_borrow_mut::<RequestContext>() {
                ctx.headers_ready();
            }
        })
    }
}
