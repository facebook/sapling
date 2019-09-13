// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::time::Instant;

use futures::{future, Future};
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::state::State;
use gotham_derive::NewMiddleware;

use crate::lfs_server_context::LoggingContext;

#[derive(Clone, NewMiddleware)]
pub struct TimerMiddleware {}

impl TimerMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

impl Middleware for TimerMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        let start_time = Instant::now();

        let f = chain(state).and_then(move |(mut state, response)| {
            if let Some(log_ctx) = state.try_borrow_mut::<LoggingContext>() {
                log_ctx.set_duration(start_time.elapsed());
            }
            future::ok((state, response))
        });

        Box::new(f)
    }
}
