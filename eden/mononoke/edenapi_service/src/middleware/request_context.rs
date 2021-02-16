/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::{
    channel::mpsc::{self, Sender},
    prelude::*,
};
use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use hyper::{Body, Response};
use load_limiter::LoadLimiterEnvironment;
use slog::{error, o, Logger};

use cloned::cloned;
use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use gotham_ext::middleware::{ClientIdentity, Middleware};
use scuba_ext::MononokeScubaSampleBuilder;
use sshrelay::Metadata;

const ERROR_CHANNEL_CAPACITY: usize = 1000;

#[derive(StateData, Clone)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub logger: Logger,
    pub error_tx: Sender<Error>,
    pub handler_error_msg: Option<String>,
}

impl RequestContext {
    async fn new(ctx: CoreContext, logger: Logger) -> Self {
        let (error_tx, mut error_rx) = mpsc::channel(ERROR_CHANNEL_CAPACITY);

        let rctx = Self {
            ctx,
            logger,
            error_tx,
            handler_error_msg: None,
        };

        // Spawn error logging task.
        //
        // NOTE: Make sure that rctx isn't cloned and then moved into this task as it will lead to
        // a memory leak due to error_rx never returning None. See D26451527 for more information.
        let _ = tokio::spawn({
            cloned!(rctx.logger);
            async move {
                while let Some(error) = error_rx.next().await {
                    error!(&logger, "{:?}", error);
                }
            }
        });

        rctx
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
    load_limiter: Option<LoadLimiterEnvironment>,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        load_limiter: Option<LoadLimiterEnvironment>,
    ) -> Self {
        Self {
            fb,
            logger,
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
        let ctx = session.new_context(logger.clone(), MononokeScubaSampleBuilder::with_discard());

        state.put(RequestContext::new(ctx, logger).await);

        None
    }
}
