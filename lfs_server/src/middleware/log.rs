/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use gotham::state::{request_id, FromState, State};
use hyper::{Body, Response};
use hyper::{Method, StatusCode, Uri, Version};
use slog::{info, o, Logger};
use time_ext::DurationExt;

use super::{ClientIdentity, Middleware, RequestContext};

#[derive(Clone)]
pub enum LogMiddleware {
    TestFriendly,
    Slog(Logger),
}

impl LogMiddleware {
    pub fn test_friendly() -> Self {
        Self::TestFriendly
    }

    pub fn slog(logger: Logger) -> Self {
        Self::Slog(logger)
    }
}

fn log_request_slog(logger: &Logger, state: &mut State, status: StatusCode) -> Option<()> {
    let uri = Uri::try_borrow_from(&state)?;
    if uri.path() == "/health_check" {
        return None;
    }
    let uri = uri.to_string();

    let method = Method::borrow_from(&state).clone();
    let version = *Version::borrow_from(&state);
    let request_id = request_id(state).to_string();
    let address = ClientIdentity::try_borrow_from(&state)
        .map(|client_identity| *client_identity.address())
        .flatten()
        .map(|addr| addr.to_string());

    let ctx = state.try_borrow_mut::<RequestContext>()?;
    let logger = logger.new(o!("request_id" => request_id));

    ctx.add_post_request(move |duration, client_hostname, bytes_sent, _| {
        info!(
            &logger,
            "{} {} \"{} {} {:?}\" {} {} - {}ms",
            address.as_ref().map(String::as_ref).unwrap_or("-"),
            client_hostname.as_ref().map(String::as_ref).unwrap_or("-"),
            method,
            uri,
            version,
            status.as_u16(),
            bytes_sent.unwrap_or(0),
            duration.as_millis_unchecked()
        );
    });

    None
}

fn log_request_test_friendly(state: &mut State, status: StatusCode) -> Option<()> {
    let method = Method::try_borrow_from(&state)?;
    let uri = Uri::try_borrow_from(&state)?;
    eprintln!("{} {} {}", method, uri, status);
    None
}

impl Middleware for LogMiddleware {
    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        match self {
            Self::TestFriendly => {
                log_request_test_friendly(state, response.status());
            }
            Self::Slog(ref logger) => {
                log_request_slog(&logger, state, response.status());
            }
        }
    }
}
