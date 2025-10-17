/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::time::Duration;

use gotham::state::FromState;
use gotham::state::State;
use http::HeaderMap;
use hyper::Body;
use hyper::Method;
use hyper::Response;
use hyper::StatusCode;
use hyper::Uri;
use hyper::Version;
use slog::Logger;
use slog::info;
use slog::o;
use time_ext::DurationExt;

use super::MetadataState;
use super::Middleware;
use super::PostResponseCallbacks;
use super::RequestLoad;
use crate::state_ext::StateExt;

const DIRECTION_REQUEST_IN: &str = "IN  >";
const DIRECTION_RESPONSE_OUT: &str = "OUT <";
const TRACE_HEADER: &str = "x-log-middleware-trace";

// We have to turn out formats into macros to avoid duplicating them:

macro_rules! SLOG_FORMAT {
    () => {
        "{} {} {} {} \"{} {} {:?}\" {} {} {} {} {}"
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
    Slog { logger: Logger, jk_name: String },
}

enum LogEntry {
    RequestIn,
    ResponseOut(StatusCode),
}

impl LogMiddleware {
    pub fn test_friendly() -> Self {
        Self::TestFriendly
    }

    pub fn slog(logger: Logger, jk_name: String) -> Self {
        Self::Slog { logger, jk_name }
    }
}

fn log_request_slog(
    logger: &Logger,
    state: &mut State,
    entry: LogEntry,
    jk_name: &str,
) -> Option<()> {
    if !justknobs::eval(jk_name, None, None).unwrap_or(false) {
        return None;
    }

    let uri = Uri::try_borrow_from(state)?;
    if uri.path() == "/health_check" {
        return None;
    }
    if uri.path() == "/proxygen/health_check" {
        return None;
    }
    let uri = uri.to_string();

    let load = *RequestLoad::borrow_from(state);
    let method = Method::borrow_from(state).clone();
    let version = *Version::borrow_from(state);
    let request_id = state.short_request_id().to_string();

    let (address, client_port) = if let Some(metadata) = MetadataState::try_borrow_from(state) {
        (
            metadata.metadata().client_ip().map(|x| x.to_string()),
            metadata.metadata().client_port().map(|x| x.to_string()),
        )
    } else if let Some(sockaddr) = gotham::state::client_addr(state) {
        (
            Some(sockaddr.ip().to_string()),
            Some(sockaddr.port().to_string()),
        )
    } else {
        (None, None)
    };

    let trace_token = HeaderMap::try_borrow_from(state)
        .and_then(|x| x.get(TRACE_HEADER))
        .and_then(|x| x.to_str().ok())
        .map(|x| x.get(..20).unwrap_or(x).to_string());

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;
    let logger = logger.new(o!("request_id" => request_id));
    match entry {
        LogEntry::RequestIn => {
            info!(
                &logger,
                SLOG_FORMAT!(),
                DIRECTION_REQUEST_IN,
                address.as_ref().map_or("-", String::as_ref),
                client_port.unwrap_or("-".to_string()),
                "-",
                method,
                uri,
                version,
                "-",
                "-",
                "-",
                load,
                trace_token.as_ref().map_or("-", String::as_ref),
            );
        }
        LogEntry::ResponseOut(status) => {
            callbacks.add(move |info| {
                info!(
                    &logger,
                    SLOG_FORMAT!(),
                    DIRECTION_RESPONSE_OUT,
                    address.as_ref().map_or("-", String::as_ref),
                    client_port.unwrap_or("-".to_string()),
                    info.client_hostname.as_ref().map_or("-", String::as_ref),
                    method,
                    uri,
                    version,
                    status.as_u16(),
                    info.meta.as_ref().map_or(0, |m| m.body().bytes_sent),
                    DurationForDisplay::from(info.duration),
                    load,
                    trace_token.as_ref().map_or("-", String::as_ref),
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
            Self::Slog { logger, jk_name } => {
                log_request_slog(logger, state, entry, jk_name);
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
