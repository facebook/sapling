/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use hyper::Body;
use hyper::Response;
use rate_limiting::RateLimitEnvironment;
use slog::o;
use slog::Logger;
use std::sync::Arc;

use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use gotham_ext::middleware::ClientIdentity;
use gotham_ext::middleware::Middleware;
use gotham_ext::state_ext::StateExt;
use metadata::Metadata;
use scuba_ext::MononokeScubaSampleBuilder;

#[derive(StateData, Clone)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub logger: Logger,
}

impl RequestContext {
    async fn new(ctx: CoreContext, logger: Logger) -> Self {
        Self { ctx, logger }
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
    scuba: Arc<MononokeScubaSampleBuilder>,
    rate_limiter: Option<RateLimitEnvironment>,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: MononokeScubaSampleBuilder,
        rate_limiter: Option<RateLimitEnvironment>,
    ) -> Self {
        Self {
            fb,
            logger,
            scuba: Arc::new(scuba),
            rate_limiter,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestContextMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let identities = ClientIdentity::borrow_from(state)
            .identities()
            .clone()
            .unwrap_or_default();

        let metadata = Metadata::default().set_identities(identities);
        let metadata = Arc::new(metadata);
        let session = SessionContainer::builder(self.fb)
            .metadata(metadata)
            .rate_limiter(self.rate_limiter.as_ref().map(|r| r.get_rate_limiter()))
            .build();

        let request_id = state.short_request_id();
        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let ctx = session.new_context(logger.clone(), (*self.scuba).clone());

        state.put(RequestContext::new(ctx, logger).await);

        None
    }
}
