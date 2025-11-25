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
use hyper::Body;
use hyper::Response;
use metadata::Metadata;
use rate_limiting::RateLimitEnvironment;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::middleware::MetadataState;
use crate::middleware::Middleware;

#[derive(StateData, Clone)]
pub struct RequestContext {
    pub ctx: CoreContext,
}

impl RequestContext {
    async fn new(ctx: CoreContext) -> Self {
        Self { ctx }
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    scuba: Arc<MononokeScubaSampleBuilder>,
    rate_limiter: Option<RateLimitEnvironment>,
    readonly: bool,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        scuba: MononokeScubaSampleBuilder,
        rate_limiter: Option<RateLimitEnvironment>,
        readonly: bool,
    ) -> Self {
        Self {
            fb,
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

        let scuba = (*self.scuba).clone().with_seq("seq");
        let ctx = session.new_context(scuba);

        state.put(RequestContext::new(ctx).await);

        None
    }
}
