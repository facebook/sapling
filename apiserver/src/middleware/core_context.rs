/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::time::Instant;

use actix_web::{
    error::Result as ActixResult,
    http::header::{HeaderMap, HeaderValue},
    middleware::{Finished, Middleware, Response, Started},
    HttpRequest, HttpResponse,
};
use anyhow::{format_err, Error};
use context::{generate_session_id, CoreContext, SessionContainer};
use fbinit::FacebookInit;
use identity::Identity;
use json_encoded::get_identities;
use openssl::x509::X509;
use percent_encoding::percent_decode;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use sshrelay::SshEnvVars;

use tracing::TraceContext;

use time_ext::DurationExt;

pub struct CoreContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
    scuba: ScubaSampleBuilder,
}

#[derive(Clone)]
enum TimeMeasurement {
    StartTime(Instant),
    ResponseTime(u64),
}

impl CoreContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: ScubaSampleBuilder,
    ) -> CoreContextMiddleware {
        CoreContextMiddleware { fb, logger, scuba }
    }

    fn start_timer<S>(&self, req: &HttpRequest<S>) {
        req.extensions_mut()
            .insert(TimeMeasurement::StartTime(Instant::now()));
    }

    fn time_cost<S>(&self, req: &HttpRequest<S>) -> Option<u64> {
        let maybe_time = req.extensions().get::<TimeMeasurement>().map(|x| x.clone());

        if let Some(time) = maybe_time {
            match time {
                TimeMeasurement::ResponseTime(t) => Some(t),
                TimeMeasurement::StartTime(t) => {
                    let cost = t.elapsed().as_micros_unchecked();
                    req.extensions_mut()
                        .insert(TimeMeasurement::ResponseTime(cost));

                    Some(cost)
                }
            }
        } else {
            None
        }
    }
}

fn extract_client_identities(cert: &X509, headers: &HeaderMap) -> Result<Vec<Identity>, Error> {
    const PROXY_IDENTITY_TYPE: &str = "SERVICE_IDENTITY";
    const PROXY_IDENTITY_DATA: &str = "proxygen";
    const PROXY_IDENTITY_HEADER: &str = "x-fb-validated-client-encoded-identity";

    let cert_identities = x509::identity::get_identities(&cert)?;

    let cert_is_trusted_proxy = cert_identities.iter().any(|identity| {
        identity.get_type() == PROXY_IDENTITY_TYPE && identity.get_data() == PROXY_IDENTITY_DATA
    });

    if !cert_is_trusted_proxy {
        return Ok(cert_identities);
    }

    let encoded_identities = headers.get(PROXY_IDENTITY_HEADER).ok_or_else(|| {
        format_err!(
            "Proxy did not provide expected header: {}",
            PROXY_IDENTITY_HEADER
        )
    })?;

    let json_identities = percent_decode(encoded_identities.as_bytes()).decode_utf8()?;

    get_identities(&json_identities).map_err(Error::from)
}

impl<S> Middleware<S> for CoreContextMiddleware {
    fn start(&self, req: &HttpRequest<S>) -> ActixResult<Started> {
        let mut scuba = self.scuba.clone();

        {
            let info = req.connection_info();
            scuba.add("hostname", info.host());
            if let Some(remote) = info.remote() {
                scuba.add("client", remote);
            }
        }

        if let Some(stream_extensions) = (*req).stream_extensions() {
            if let Some(cert) = (*stream_extensions).get::<X509>() {
                if let Ok(identities) = extract_client_identities(&cert, req.headers()) {
                    let identities: Vec<_> =
                        identities.into_iter().map(|i| i.to_string()).collect();
                    scuba.add("client_identities", identities.join(","));
                    scuba.add("client_identities_normvector", identities);
                }
            }
        }

        let session_id = generate_session_id();
        let repo_name = req.path().split("/").nth(1).unwrap_or("unknown");

        scuba
            .add("type", "http")
            .add("method", req.method().to_string())
            .add("path", req.path())
            .add("reponame", repo_name)
            .add("session_uuid", session_id.to_string());

        let session = SessionContainer::new(
            self.fb,
            session_id,
            TraceContext::default(self.fb),
            None,
            None,
            None,
            SshEnvVars::default(),
            None,
        );

        let ctx = session.new_context(self.logger.clone(), scuba);

        req.extensions_mut().insert(ctx);
        self.start_timer(req);

        Ok(Started::Done)
    }

    fn response(&self, req: &HttpRequest<S>, mut resp: HttpResponse) -> ActixResult<Response> {
        if let Some(ctx) = req.extensions_mut().get_mut::<CoreContext>() {
            if let Ok(session_header) = HeaderValue::from_str(&ctx.session_id().to_string()) {
                resp.headers_mut().insert("X-Session-ID", session_header);
            }
        }

        Ok(Response::Done(resp))
    }

    fn finish(&self, req: &HttpRequest<S>, resp: &HttpResponse) -> Finished {
        let response_time = self.time_cost(req);

        if let Some(ctx) = req.extensions_mut().get_mut::<CoreContext>() {
            let mut scuba = ctx.scuba().clone();
            scuba.add("status_code", resp.status().as_u16());
            scuba.add("response_size", resp.response_size());
            scuba.add("log_tag", "HTTP request finished");

            if let Some(time) = response_time {
                scuba.add("response_time", time);
            }

            scuba.log();
        }

        info!(
            self.logger,
            "{} {} {} {:.3}\u{00B5}s",
            resp.status().as_u16(),
            req.method(),
            req.path(),
            response_time.unwrap_or(0),
        );

        Finished::Done
    }
}
