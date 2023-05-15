/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::anyhow;
use anyhow::Result;
use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::NewMiddleware;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::MetadataState;
use gotham_ext::response::build_error_response;
use http::HeaderMap;
use hyper::Uri;
use qps::Qps;
use slog::trace;

use super::error_formatter::LfsErrorFormatter;
use crate::config::ServerConfig;
use crate::LfsServerContext;

const HEADER_REVPROXY_REGION: &str = "x-fb-revproxy-region";

// NOTE: Our Throttling middleware is implemented as Gotham middleware for 3 reasons:
// - It needs to replace responses.
// - It needs to do asynchronously.
// - It only needs to run if we're going to serve a request.

#[derive(Clone, NewMiddleware)]
pub struct ThrottleMiddleware {
    fb: FacebookInit,
    handle: ConfigHandle<ServerConfig>,
}

impl ThrottleMiddleware {
    pub fn new(fb: FacebookInit, handle: ConfigHandle<ServerConfig>) -> Self {
        Self { fb, handle }
    }
}

impl Middleware for ThrottleMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Pin<Box<HandlerFuture>>
    where
        Chain: FnOnce(State) -> Pin<Box<HandlerFuture>>,
    {
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return chain(state);
            }
        }
        let identities = state
            .try_borrow::<MetadataState>()
            .map(|metadata_state| metadata_state.metadata().identities());

        for limit in self.handle.get().loadshedding_limits().iter() {
            if let Err(err) = limit.should_load_shed(self.fb, identities) {
                let err = HttpError::e429(err);

                let res =
                    async move { build_error_response(err, state, &LfsErrorFormatter) }.boxed();

                return res;
            }
        }

        chain(state)
    }
}

#[derive(Clone, NewMiddleware)]
pub struct QpsMiddleware {
    lfs_ctx: LfsServerContext,
}

impl QpsMiddleware {
    pub fn new(lfs_ctx: LfsServerContext) -> Self {
        Self { lfs_ctx }
    }
}

impl Middleware for QpsMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Pin<Box<HandlerFuture>>
    where
        Chain: FnOnce(State) -> Pin<Box<HandlerFuture>>,
    {
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return chain(state);
            }
        }

        let headers = HeaderMap::try_borrow_from(&state).expect("No headers in the request");

        if let Err(e) = { bump_qps(headers, &self.lfs_ctx.qps()) } {
            trace!(self.lfs_ctx.logger(), "Failed to bump QPS: {:?}", e);
        }

        chain(state)
    }
}

fn bump_qps(headers: &HeaderMap, qps: &Option<Qps>) -> Result<()> {
    let qps = match qps {
        Some(qps) => qps,
        None => return Ok(()),
    };
    match headers.get(HEADER_REVPROXY_REGION) {
        Some(proxy_region) => {
            qps.bump(proxy_region.to_str()?)?;
            Ok(())
        }
        None => Err(anyhow!("No {:?} header.", HEADER_REVPROXY_REGION)),
    }
}
