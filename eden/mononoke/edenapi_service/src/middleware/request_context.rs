/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use hyper::{Body, Response};
use load_limiter::LoadLimiterEnvironment;
use slog::{o, Logger};
use std::sync::Arc;

use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use gotham_ext::middleware::{ClientIdentity, Middleware};
use scuba_ext::MononokeScubaSampleBuilder;
use sshrelay::Metadata;

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
    load_limiter: Option<LoadLimiterEnvironment>,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: MononokeScubaSampleBuilder,
        load_limiter: Option<LoadLimiterEnvironment>,
    ) -> Self {
        Self {
            fb,
            logger,
            scuba: Arc::new(scuba),
            load_limiter,
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

        let load_limiter = self.load_limiter.as_ref().map(|l| l.get(&identities, None));
        let metadata = Metadata::default().set_identities(identities);
        let session = SessionContainer::builder(self.fb)
            .metadata(metadata)
            .load_limiter(load_limiter)
            .build();

        let request_id = request_id(&state);
        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let ctx = session.new_context(logger.clone(), (*self.scuba).clone());

        state.put(RequestContext::new(ctx, logger).await);

        None
    }
}
