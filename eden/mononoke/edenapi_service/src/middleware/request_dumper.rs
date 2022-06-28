/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use hyper::Body;
use hyper::Response;

use crate::middleware::RequestContext;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use base64::encode as b64encode;
use bytes::Bytes;
use fbinit::FacebookInit;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::middleware::Middleware;
use http::HeaderMap;
use lazy_static::lazy_static;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::trace;
use slog::warn;
use std::collections::HashSet;
use tunables::tunables;

static MAX_BODY_LEN: usize = 16 * 1024; // 16 KB

lazy_static! {
    static ref FILTERED_HEADERS: HashSet<&'static str> = {
        let mut m = HashSet::new();
        m.insert("x-auth-cats");
        m
    };
}

#[derive(Debug, StateData, Clone, PartialEq)]
enum LogAction {
    Log,
    BodyTooBig,
}

#[derive(Debug, StateData, Clone)]
pub struct RequestDumper {
    logger: MononokeScubaSampleBuilder,
    log_action: LogAction,
}

fn get_content_len(headers: &HeaderMap) -> Option<usize> {
    let content_len = headers.get(http::header::CONTENT_LENGTH)?;
    let cl: Result<usize> = try { content_len.to_str()?.parse()? };
    cl.ok()
}

impl RequestDumper {
    pub fn add_http_req_prefix(&mut self, state: &State, headers: &HeaderMap) -> Result<()> {
        let method = http::method::Method::try_borrow_from(state)
            .context("Method not present in State")?
            .as_str();
        self.logger.add("method", method);

        let uri = http::uri::Uri::try_borrow_from(state).context("Uri not present in State")?;
        self.logger.add(
            "path",
            uri.path_and_query()
                .context("path_and_query is None")?
                .as_str(),
        );

        let mut headers_hs = HashSet::new();
        for (k, v) in headers
            .iter()
            .filter(|(k, _v)| !FILTERED_HEADERS.contains(k.as_str()))
        {
            headers_hs.insert(format!("{}: {}", k.as_str(), v.to_str()?));
        }
        self.logger.add("headers", headers_hs);
        Ok(())
    }

    fn should_log(&self) -> bool {
        match self.log_action {
            LogAction::Log => true,
            LogAction::BodyTooBig => false,
        }
    }

    pub fn log(&mut self) -> Result<()> {
        if !self.should_log() {
            bail!(
                "Shouldn't log this request. Either sampled or {:?}",
                self.log_action
            )
        }
        if !self.logger.log() {
            bail!("failed to dump request")
        }
        Ok(())
    }

    pub fn add_body(&mut self, body: &Bytes) {
        if body.len() > MAX_BODY_LEN {
            self.log_action = LogAction::BodyTooBig;
            return;
        }

        self.logger.add("body", b64encode(&body[..]));
    }

    pub fn new(fb: FacebookInit) -> Self {
        let scuba = MononokeScubaSampleBuilder::new(fb, "mononoke_replay_logged_edenapi_requests");
        Self {
            logger: scuba,
            log_action: LogAction::Log,
        }
    }
}

#[derive(Clone)]
pub struct RequestDumperMiddleware {
    fb: FacebookInit,
}

impl RequestDumperMiddleware {
    pub fn new(fb: FacebookInit) -> Self {
        Self { fb }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestDumperMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let logger = &RequestContext::borrow_from(state).logger;
        let sample_ratio: u64 = match tunables().get_edenapi_req_dumper_sample_ratio().try_into() {
            Ok(n) => n,
            Err(e) => {
                warn!(
                    logger,
                    "Couldn't read edenapi_req_dumper_sample_ratio tunable: {}", e
                );
                return None;
            }
        };

        if sample_ratio == 0 || (rand::random::<u64>() % sample_ratio) != 0 {
            trace!(logger, "Won't record this request");
            return None;
        }

        let headers = match HeaderMap::try_borrow_from(state).context("No headers in State") {
            Ok(headers) => headers,
            Err(e) => {
                warn!(logger, "Error when borrowing headers from State: {}", e);
                return None;
            }
        };

        if let Some(len) = get_content_len(headers) {
            if len > MAX_BODY_LEN {
                trace!(logger, "Body too big ({}), not recording", len);
                return None;
            }
        }

        let mut rd = RequestDumper::new(self.fb);

        if let Err(e) = rd.add_http_req_prefix(state, headers) {
            warn!(
                logger,
                "Err while attempting to record http req prefix: {}", e
            );
            return None;
        }

        state.put(rd);

        None
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        if let Some(rd) = RequestDumper::try_borrow_mut_from(state) {
            if let Err(e) = rd.log() {
                let rctx = RequestContext::borrow_from(state);
                warn!(rctx.logger, "Couldn't dump request: {}", e);
            }
        }
    }
}
