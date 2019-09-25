// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use hyper::{
    header::{self, AsHeaderName, HeaderMap},
    Method, StatusCode, Uri,
};
use hyper::{Body, Response};
use scuba::{ScubaSampleBuilder, ScubaValue};
use time_ext::DurationExt;

use super::{ClientIdentity, Middleware, RequestContext};

#[derive(Clone)]
pub struct ScubaMiddleware {
    scuba: ScubaSampleBuilder,
}

impl ScubaMiddleware {
    pub fn new(scuba: ScubaSampleBuilder) -> Self {
        Self { scuba }
    }
}

fn add_header<'a, T: AsHeaderName>(
    scuba: &mut ScubaSampleBuilder,
    headers: &'a HeaderMap,
    scuba_key: &str,
    header: T,
) -> Option<&'a str> {
    if let Some(header_val) = headers.get(header) {
        if let Ok(header_val) = header_val.to_str() {
            scuba.add(scuba_key, header_val.to_string());
            return Some(header_val);
        }
    }

    None
}

fn log_stats(state: &mut State, status_code: &StatusCode) -> Option<()> {
    let mut scuba = state.try_take::<ScubaMiddlewareState>()?.0;

    scuba.add("http_status", status_code.as_u16());

    if let Some(uri) = Uri::try_borrow_from(&state) {
        scuba.add("http_path", uri.path());
    }

    if let Some(method) = Method::try_borrow_from(&state) {
        scuba.add("http_method", method.to_string());
    }

    if let Some(headers) = HeaderMap::try_borrow_from(&state) {
        add_header(&mut scuba, headers, "http_host", header::HOST);
    }

    if let Some(identity) = ClientIdentity::try_borrow_from(&state) {
        if let Some(ref address) = identity.address() {
            scuba.add("client_ip", address.to_string());
        }

        if let Some(ref client_correlator) = identity.client_correlator() {
            scuba.add("client_correlator", client_correlator.to_string());
        }

        if let Some(ref identities) = identity.identities() {
            let identities: Vec<_> = identities.into_iter().map(|i| i.to_string()).collect();
            scuba.add("client_identities", identities);
        }
    }

    scuba.add("request_id", request_id(&state));

    let ctx = state.try_borrow_mut::<RequestContext>()?;

    if let Some(repository) = &ctx.repository {
        scuba.add("repository", repository.as_ref());
    }

    if let Some(method) = ctx.method {
        scuba.add("method", method.to_string());
    }

    if let Some(err_msg) = &ctx.error_msg {
        scuba.add("error_msg", err_msg.as_ref());
    }

    if let Some(response_size) = ctx.response_size {
        scuba.add("response_size", response_size);
    }

    if let Some(headers_duration) = ctx.headers_duration {
        scuba.add(
            "headers_duration_ms",
            headers_duration.as_millis_unchecked(),
        );
    }

    ctx.add_post_request(move |duration, client_hostname| {
        scuba.add("duration_ms", duration.as_millis_unchecked());
        if let Some(client_hostname) = client_hostname {
            scuba.add("client_hostname", client_hostname.to_string());
        }
        scuba.log();
    });

    Some(())
}

#[derive(StateData)]
pub struct ScubaMiddlewareState(ScubaSampleBuilder);

impl ScubaMiddlewareState {
    pub fn add<K: Into<String>, V: Into<ScubaValue>>(&mut self, key: K, value: V) -> &mut Self {
        self.0.add(key, value);
        self
    }
}

impl Middleware for ScubaMiddleware {
    fn inbound(&self, state: &mut State) {
        state.put(ScubaMiddlewareState(self.scuba.clone()));
    }

    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return;
            }
        }

        log_stats(state, &response.status());
    }
}
