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
use json_encoded::get_identities;
use percent_encoding::percent_decode;
use scuba::{ScubaSampleBuilder, ScubaValue};
use time_ext::DurationExt;

use super::{Callback, Middleware, RequestContext};

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";
const CLIENT_CORRELATOR: &str = "x-client-correlator";

#[derive(Clone)]
pub struct ScubaMiddleware {
    scuba: ScubaSampleBuilder,
}

impl ScubaMiddleware {
    pub fn new(scuba: ScubaSampleBuilder) -> Self {
        Self { scuba }
    }
}

fn add_header<T: AsHeaderName>(
    scuba: &mut ScubaSampleBuilder,
    headers: &HeaderMap,
    scuba_key: &str,
    header: T,
) {
    if let Some(header_val) = headers.get(header) {
        if let Ok(header_val) = header_val.to_str() {
            scuba.add(scuba_key, header_val.to_string());
        }
    }
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
        add_header(&mut scuba, headers, "client_ip", CLIENT_IP);
        add_header(&mut scuba, headers, "client_correlator", CLIENT_CORRELATOR);

        // NOTE: We decode the identity here, but that's only indicative since we don't
        // verify the TLS peer's identity, so we don't know if we can trust this header.
        if let Some(encoded_client_identity) = headers.get(ENCODED_CLIENT_IDENTITY) {
            let identities = percent_decode(encoded_client_identity.as_bytes())
                .decode_utf8()
                .map_err(|_| ())
                .and_then(|decoded| get_identities(decoded.as_ref()).map_err(|_| ()));

            if let Ok(identities) = identities {
                let identities: Vec<_> = identities.into_iter().map(|i| i.to_string()).collect();
                scuba.add("client_identities", identities);
            }
        }
    }

    scuba.add("request_id", request_id(&state));

    let ctx = state.try_borrow_mut::<RequestContext>()?;

    if let Some(repository) = &ctx.repository {
        scuba.add("repository", repository.as_ref());
    }

    if let Some(method) = ctx.method {
        scuba.add("method", method);
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

    ctx.add_post_request(move |duration| {
        scuba
            .add("duration_ms", duration.as_millis_unchecked())
            .log();
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
    fn handle(&self, state: &mut State) -> Callback {
        state.put(ScubaMiddlewareState(self.scuba.clone()));

        Box::new(|state, response| {
            if let Some(uri) = Uri::try_borrow_from(&state) {
                if uri.path() == "/health_check" {
                    return;
                }
            }

            log_stats(state, &response.status());
        })
    }
}
