/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::{request_id, State};
use gotham_derive::StateData;
use slog::{o, Logger};

use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use gotham_ext::middleware::Middleware;
use scuba::ScubaSampleBuilder;

#[derive(StateData)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub repository: Option<String>,
}

impl RequestContext {
    fn new(ctx: CoreContext) -> Self {
        Self {
            ctx,
            repository: None,
        }
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
}

impl RequestContextMiddleware {
    pub fn new(fb: FacebookInit, logger: Logger) -> Self {
        Self { fb, logger }
    }
}

impl Middleware for RequestContextMiddleware {
    fn inbound(&self, state: &mut State) {
        let request_id = request_id(&state);

        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(logger, ScubaSampleBuilder::with_discard());

        state.put(RequestContext::new(ctx));
    }
}
