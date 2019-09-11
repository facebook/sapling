// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::time::Instant;

use futures::{future, Future};
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::state::{request_id, FromState, State};
use gotham_derive::NewMiddleware;
use hyper::{Method, Uri};
use scuba::ScubaSampleBuilder;
use time_ext::DurationExt;

use crate::lfs_server_context::LoggingContext;

#[derive(Clone, NewMiddleware)]
pub struct ScubaMiddleware {
    scuba: ScubaSampleBuilder,
}

impl ScubaMiddleware {
    pub fn new(scuba: ScubaSampleBuilder) -> Self {
        Self { scuba }
    }
}

impl Middleware for ScubaMiddleware {
    fn call<Chain>(mut self, state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        // Don't log health check requests.
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return chain(state);
            }
        }

        let start_time = Instant::now();

        let f = chain(state).and_then(move |(mut state, response)| {
            let log_ctx = state.try_take::<LoggingContext>();

            if let Some(log_ctx) = log_ctx {
                self.scuba.add("repository", log_ctx.repository);

                if let Some(err_msg) = log_ctx.error_msg {
                    self.scuba.add("error_msg", err_msg);
                }
            }

            if let Some(uri) = Uri::try_borrow_from(&state) {
                self.scuba.add("http_path", uri.path());
            }

            if let Some(method) = Method::try_borrow_from(&state) {
                self.scuba.add("http_method", method.to_string());
            }

            self.scuba
                .add("http_status", response.status().as_u16())
                .add("request_id", request_id(&state))
                .add("duration_ms", start_time.elapsed().as_millis_unchecked());

            self.scuba.log();
            future::ok((state, response))
        });

        Box::new(f)
    }
}
