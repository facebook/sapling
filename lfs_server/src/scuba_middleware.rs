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
use hyper::{
    header::{self, HeaderMap},
    Method, Uri,
};
use json_encoded::get_identities;
use percent_encoding::percent_decode;
use scuba::ScubaSampleBuilder;
use time_ext::DurationExt;

use crate::lfs_server_context::LoggingContext;

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";

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

            if let Some(headers) = HeaderMap::try_borrow_from(&state) {
                if let Some(http_host) = headers.get(header::HOST) {
                    if let Ok(http_host) = http_host.to_str() {
                        self.scuba.add("http_host", http_host.to_string());
                    }
                }

                if let Some(client_ip) = headers.get(CLIENT_IP) {
                    if let Ok(client_ip) = client_ip.to_str() {
                        self.scuba.add("client_ip", client_ip.to_string());
                    }
                }

                // NOTE: We decode the identity here, but that's only indicative since we don't
                // verify the TLS peer's identity, so we don't know if we can trust this header.
                if let Some(encoded_client_identity) = headers.get(ENCODED_CLIENT_IDENTITY) {
                    let identities = percent_decode(encoded_client_identity.as_bytes())
                        .decode_utf8()
                        .map_err(|_| ())
                        .and_then(|decoded| get_identities(decoded.as_ref()).map_err(|_| ()));

                    if let Ok(identities) = identities {
                        let identities: Vec<_> =
                            identities.into_iter().map(|i| i.to_string()).collect();
                        self.scuba.add("client_identities", identities);
                    }
                }
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
