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
use std::fmt::{self, Debug, Display};
use time_ext::DurationExt;

use super::{ClientIdentity, Middleware, RequestContext, RequestLoad};

const DIRECTION_REQUEST_IN: &str = "IN  >";
const DIRECTION_RESPONSE_OUT: &str = "OUT <";

// We have to turn out formats into macros to avoid duplicating them:

macro_rules! SLOG_FORMAT {
    () => {
        "{} {} {} \"{} {} {:?}\" {} {} {} {}"
    };
}

macro_rules! TEST_FRIENDLY_FORMAT {
    () => {
        "{} {} {} {}"
    };
}

/// We use DurationForDisplay to append ms on non-empty durations.
#[derive(Debug)]
struct DurationForDisplay(u64);

impl Display for DurationForDisplay {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, fmt)?;
        write!(fmt, "ms")
    }
}

#[derive(Clone)]
pub enum LogMiddleware {
    TestFriendly,
    Slog(Logger),
}

enum LogEntry {
    RequestIn,
    ResponseOut(StatusCode),
}

impl LogMiddleware {
    pub fn test_friendly() -> Self {
        Self::TestFriendly
    }

    pub fn slog(logger: Logger) -> Self {
        Self::Slog(logger)
    }
}

fn log_request_slog(logger: &Logger, state: &mut State, entry: LogEntry) -> Option<()> {
    let uri = Uri::try_borrow_from(&state)?;
    if uri.path() == "/health_check" {
        return None;
    }
    let uri = uri.to_string();

    let load = *RequestLoad::borrow_from(&state);
    let method = Method::borrow_from(&state).clone();
    let version = *Version::borrow_from(&state);
    let request_id = request_id(state).to_string();
    let address = ClientIdentity::try_borrow_from(&state)
        .map(|client_identity| *client_identity.address())
        .flatten()
        .map(|addr| addr.to_string());

    let ctx = state.try_borrow_mut::<RequestContext>()?;
    let logger = logger.new(o!("request_id" => request_id));

    match entry {
        LogEntry::RequestIn => {
            info!(
                &logger,
                SLOG_FORMAT!(),
                DIRECTION_REQUEST_IN,
                address.as_ref().map(String::as_ref).unwrap_or("-"),
                "-",
                method,
                uri,
                version,
                "-",
                "-",
                "-",
                load,
            );
        }
        LogEntry::ResponseOut(status) => {
            ctx.add_post_request(move |duration, client_hostname, bytes_sent, _| {
                info!(
                    &logger,
                    SLOG_FORMAT!(),
                    DIRECTION_RESPONSE_OUT,
                    address.as_ref().map(String::as_ref).unwrap_or("-"),
                    client_hostname.as_ref().map(String::as_ref).unwrap_or("-"),
                    method,
                    uri,
                    version,
                    status.as_u16(),
                    bytes_sent.unwrap_or(0),
                    DurationForDisplay(duration.as_millis_unchecked()),
                    load,
                );
            });
        }
    }

    None
}

fn log_request_test_friendly(state: &mut State, entry: LogEntry) -> Option<()> {
    let method = Method::try_borrow_from(&state)?;
    let uri = Uri::try_borrow_from(&state)?;

    match entry {
        LogEntry::RequestIn => {
            eprintln!(
                TEST_FRIENDLY_FORMAT!(),
                DIRECTION_REQUEST_IN, method, uri, "-"
            );
        }
        LogEntry::ResponseOut(status) => {
            eprintln!(
                TEST_FRIENDLY_FORMAT!(),
                DIRECTION_RESPONSE_OUT, method, uri, status
            );
        }
    };

    None
}

impl LogMiddleware {
    fn log(&self, state: &mut State, entry: LogEntry) {
        match self {
            Self::TestFriendly => {
                log_request_test_friendly(state, entry);
            }
            Self::Slog(ref logger) => {
                log_request_slog(&logger, state, entry);
            }
        }
    }
}

impl Middleware for LogMiddleware {
    fn inbound(&self, state: &mut State) {
        let entry = LogEntry::RequestIn;
        self.log(state, entry);
    }

    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        let entry = LogEntry::ResponseOut(response.status());
        self.log(state, entry);
    }
}
