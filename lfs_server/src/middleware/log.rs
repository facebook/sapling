// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::{client_addr, request_id, FromState, State};
use hyper::{Body, Response};
use hyper::{Method, StatusCode, Uri, Version};
use slog::{info, Logger};
use time_ext::DurationExt;

use super::{Middleware, RequestContext};

#[derive(Clone)]
pub struct LogMiddleware {
    logger: Logger,
}

impl LogMiddleware {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}

fn log_request(logger: &Logger, state: &State, status: &StatusCode) -> Option<()> {
    let uri = Uri::try_borrow_from(&state)?;
    if uri.path() == "/health_check" {
        return None;
    }

    let ctx = state.try_borrow::<RequestContext>()?;
    let headers_duration = ctx.headers_duration?;
    let response_size = ctx.response_size?;

    let request_id = request_id(state);
    let ip = client_addr(&state)?.ip();

    let method = Method::borrow_from(&state);
    let version = Version::borrow_from(&state);

    // log out
    info!(
        logger,
        "{} - \"{} {} {:?}\" {} {} - {}ms",
        ip,
        method,
        uri,
        version,
        status,
        response_size,
        headers_duration.as_millis_unchecked();
        "request_id" => request_id,
    );

    None
}

impl Middleware for LogMiddleware {
    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        log_request(&self.logger, &state, &response.status());
    }
}
