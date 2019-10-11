/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::time::Instant;

use actix_web::{
    error::Result,
    http::header::HeaderValue,
    middleware::{Finished, Middleware, Response, Started},
    HttpRequest, HttpResponse,
};
use context::CoreContext;
use fbinit::FacebookInit;
use openssl::x509::X509;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use sshrelay::SshEnvVars;
use uuid::Uuid;
use x509::identity;

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

impl<S> Middleware<S> for CoreContextMiddleware {
    fn start(&self, req: &HttpRequest<S>) -> Result<Started> {
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
                if let Ok(identities) = identity::get_identities(&cert) {
                    scuba.add(
                        "client_identities",
                        identities
                            .into_iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
            }
        }

        let session_uuid = Uuid::new_v4();
        let repo_name = req.path().split("/").nth(1).unwrap_or("unknown");

        scuba
            .add("type", "http")
            .add("method", req.method().to_string())
            .add("path", req.path())
            .add("reponame", repo_name)
            .add("session_uuid", session_uuid.to_string());

        let ctx = CoreContext::new(
            self.fb,
            session_uuid,
            self.logger.clone(),
            scuba,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        );

        req.extensions_mut().insert(ctx);
        self.start_timer(req);

        Ok(Started::Done)
    }

    fn response(&self, req: &HttpRequest<S>, mut resp: HttpResponse) -> Result<Response> {
        if let Some(ctx) = req.extensions_mut().get_mut::<CoreContext>() {
            if let Ok(session_header) = HeaderValue::from_str(&ctx.session().to_string()) {
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
