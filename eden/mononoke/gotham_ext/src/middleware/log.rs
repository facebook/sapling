/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::FromState;
use gotham::state::State;
use hyper::Body;
use hyper::Method;
use hyper::Response;
use hyper::StatusCode;
use hyper::Uri;
use hyper::Version;
use slog::info;
use slog::o;
use slog::Logger;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::time::Duration;
use time_ext::DurationExt;

use super::ClientIdentity;
use super::Middleware;
use super::PostResponseCallbacks;
use super::RequestLoad;
use crate::state_ext::StateExt;

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
struct DurationForDisplay(Option<u64>);

impl From<Option<Duration>> for DurationForDisplay {
    fn from(duration: Option<Duration>) -> Self {
        Self(duration.map(|d| d.as_millis_unchecked()))
    }
}

impl Display for DurationForDisplay {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(duration) => {
                fmt::Display::fmt(&duration, fmt)?;
                write!(fmt, "ms")
            }
            None => write!(fmt, "-"),
        }
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
    let uri = Uri::try_borrow_from(state)?;
    if uri.path() == "/health_check" {
        return None;
    }
    let uri = uri.to_string();

    let load = *RequestLoad::borrow_from(state);
    let method = Method::borrow_from(state).clone();
    let version = *Version::borrow_from(state);
    let request_id = state.short_request_id().to_string();
    let address = ClientIdentity::try_borrow_from(state)
        .and_then(|client_identity| *client_identity.address())
        .map(|addr| addr.to_string());

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;
    let logger = logger.new(o!("request_id" => request_id));

    match entry {
        LogEntry::RequestIn => {
            info!(
                &logger,
                SLOG_FORMAT!(),
                DIRECTION_REQUEST_IN,
                address.as_ref().map_or("-", String::as_ref),
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
            callbacks.add(move |info| {
                info!(
                    &logger,
                    SLOG_FORMAT!(),
                    DIRECTION_RESPONSE_OUT,
                    address.as_ref().map_or("-", String::as_ref),
                    info.client_hostname.as_ref().map_or("-", String::as_ref),
                    method,
                    uri,
                    version,
                    status.as_u16(),
                    info.meta.as_ref().map_or(0, |m| m.body().bytes_sent),
                    DurationForDisplay::from(info.duration),
                    load,
                );
            });
        }
    }

    None
}

fn log_request_test_friendly(state: &mut State, entry: LogEntry) -> Option<()> {
    let method = Method::try_borrow_from(state)?;
    let uri = Uri::try_borrow_from(state)?;

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
                log_request_slog(logger, state, entry);
            }
        }
    }
}

#[async_trait::async_trait]
impl Middleware for LogMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let entry = LogEntry::RequestIn;
        self.log(state, entry);
        None
    }

    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        let entry = LogEntry::ResponseOut(response.status());
        self.log(state, entry);
    }
}
