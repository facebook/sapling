/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::time::{Duration, Instant};

use cached_config::ConfigHandle;
use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use futures::{
    channel::oneshot::{self, Receiver, Sender},
    prelude::*,
};
use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use gotham_ext::{
    middleware::{ClientIdentity, Middleware},
    response::ResponseContentLength,
};
use hyper::{body::Body, Response};
use scuba::ScubaSampleBuilder;
use slog::{o, Logger};
use tokio::task;

use crate::config::ServerConfig;

use super::RequestStartTime;

type PostRequestCallback =
    Box<dyn FnOnce(&Option<Duration>, &Option<String>, Option<u64>) + Sync + Send + 'static>;

#[derive(Copy, Clone)]
pub enum LfsMethod {
    Upload,
    Download,
    DownloadSha256,
    Batch,
}

impl fmt::Display for LfsMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::DownloadSha256 => "download_sha256",
            Self::Batch => "batch",
        };
        write!(f, "{}", name)
    }
}

#[derive(StateData)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub repository: Option<String>,
    pub method: Option<LfsMethod>,
    pub error_msg: Option<String>,
    pub should_log: bool,

    checkpoint: Option<Receiver<u64>>,
    post_request_callbacks: Vec<PostRequestCallback>,
}

impl RequestContext {
    fn new(ctx: CoreContext, should_log: bool) -> Self {
        Self {
            ctx,
            repository: None,
            method: None,
            error_msg: None,
            should_log,
            checkpoint: None,
            post_request_callbacks: vec![],
        }
    }

    pub fn set_request(&mut self, repository: String, method: LfsMethod) {
        self.repository = Some(repository);
        self.method = Some(method);
    }

    pub fn set_error_msg(&mut self, error_msg: String) {
        self.error_msg = Some(error_msg);
    }

    pub fn add_post_request<T>(&mut self, callback: T)
    where
        T: FnOnce(&Option<Duration>, &Option<String>, Option<u64>) + Sync + Send + 'static,
    {
        self.post_request_callbacks.push(Box::new(callback));
    }

    /// Delay post request until a callback has completed. This is useful to e.g. record how much data was sent.
    pub fn delay_post_request(&mut self) -> Sender<u64> {
        // NOTE: If this is called twice ... then the first one will be ignored
        let (sender, receiver) = oneshot::channel();
        self.checkpoint = Some(receiver);
        sender
    }

    fn dispatch_post_request<H>(
        self,
        start_time: Option<Instant>,
        content_length: Option<u64>,
        client_hostname: H,
    ) where
        H: Future<Output = Option<String>> + Send + 'static,
    {
        let Self {
            post_request_callbacks,
            checkpoint,
            ..
        } = self;

        let fut = async move {
            let bytes_sent = if let Some(checkpoint) = checkpoint {
                // NOTE: We don't use await? here, because we want to run callbacks even if the
                // receiver was dropped!
                checkpoint.await.map(Some).unwrap_or(None)
            } else {
                content_length
            };

            // Capture elapsed time before waiting for the client hostname to resolve.
            let elapsed = start_time.map(|start| start.elapsed());

            // Resolve client hostname. Querying DNS might be slow.
            let client_hostname = client_hostname.await;

            for callback in post_request_callbacks.into_iter() {
                callback(&elapsed, &client_hostname, bytes_sent)
            }
        };

        task::spawn(fut);
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
    config_handle: ConfigHandle<ServerConfig>,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        config_handle: ConfigHandle<ServerConfig>,
    ) -> Self {
        Self {
            fb,
            logger,
            config_handle,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestContextMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let request_id = request_id(&state);

        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(logger, ScubaSampleBuilder::with_discard());

        let should_log = ClientIdentity::try_borrow_from(&state)
            .map(|client_identity| !client_identity.is_proxygen_test_identity())
            .unwrap_or(true);

        state.put(RequestContext::new(ctx, should_log));

        None
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        let start_time = RequestStartTime::try_borrow_from(&state).map(|t| t.0);
        let content_length = ResponseContentLength::try_borrow_from(&state).map(|l| l.0);

        let config = self.config_handle.get();
        let client_identity = ClientIdentity::try_borrow_from(&state);
        let client_hostname = match client_identity {
            Some(id) if !config.disable_hostname_logging() => id.hostname().left_future(),
            _ => future::ready(None).right_future(),
        };

        if let Some(ctx) = state.try_take::<RequestContext>() {
            ctx.dispatch_post_request(start_time, content_length, client_hostname);
        }
    }
}
