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
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use http::Response;
use http::header::HeaderMap;
use metadata::Metadata;
use rate_limiting::RateLimitEnvironment;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::middleware::MetadataState;
use crate::middleware::Middleware;

const ENFORCE_PATH_ACLS_HEADER: &str = "X-Enforce-Path-Acls";

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

        let enforce_path_acls = HeaderMap::try_borrow_from(state)
            .and_then(|headers| headers.get(ENFORCE_PATH_ACLS_HEADER))
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "true" || v == "1");

        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .readonly(self.readonly)
            .rate_limiter(self.rate_limiter.as_ref().map(|r| r.get_rate_limiter()))
            .server_side_tenting(enforce_path_acls)
            .build();

        let scuba = (*self.scuba).clone().with_seq("seq");
        let ctx = session.new_context(scuba);
        ctx.track_cpu_time();

        state.put(RequestContext::new(ctx).await);

        None
    }
}
