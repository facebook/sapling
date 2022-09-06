/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use gotham_ext::state_ext::StateExt;
use hyper::Body;
use hyper::Response;
use metadata::Metadata;
use rate_limiting::RateLimitEnvironment;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::o;
use slog::Logger;

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
    readonly: bool,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: MononokeScubaSampleBuilder,
        rate_limiter: Option<RateLimitEnvironment>,
        readonly: bool,
    ) -> Self {
        Self {
            fb,
            logger,
            scuba: Arc::new(scuba),
            rate_limiter,
            readonly,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestContextMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let metadata = if let Some(metadata_state) = MetadataState::try_borrow_from(state) {
            metadata_state.metadata().clone()
        } else {
            Metadata::default()
        };

        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .readonly(self.readonly)
            .rate_limiter(self.rate_limiter.as_ref().map(|r| r.get_rate_limiter()))
            .build();

        let request_id = state.short_request_id();
        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let ctx = session.new_context(logger.clone(), (*self.scuba).clone());

        state.put(RequestContext::new(ctx, logger).await);

        None
    }
}
