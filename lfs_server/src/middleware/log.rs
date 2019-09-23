// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::{client_addr, request_id, FromState, State};
use hyper::{Method, StatusCode, Uri, Version};
use slog::{info, Logger};
use std::sync::Arc;
use time_ext::DurationExt;

use super::{Callback, Middleware, RequestContext};

#[derive(Clone)]
pub struct LogMiddleware {
    logger: Arc<Logger>,
}

impl LogMiddleware {
    pub fn new(logger: Logger) -> Self {
        Self {
            logger: Arc::new(logger),
        }
    }
}

fn log_request(logger: &Logger, state: &State, status: &StatusCode) -> Option<()> {
    let uri = Uri::try_borrow_from(&state)?;
    if uri.path() == "/health_check" {
        return None;
    }

    let log_ctx = state.try_borrow::<RequestContext>()?;
    let duration = log_ctx.duration?;
    let response_size = log_ctx.response_size?;

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
        duration.as_millis_unchecked();
        "request_id" => request_id,
    );

    None
}

impl Middleware for LogMiddleware {
    fn handle(&self, _state: &mut State) -> Callback {
        let logger = self.logger.clone();
        Box::new(move |state, response| {
            log_request(&logger, &state, &response.status());
        })
    }
}
