/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;

use anyhow::Error;
use configerator_cached::ConfigHandle;
use context::{CoreContext, PerfCounters, SessionContainer};
use dns_lookup::lookup_addr;
use fbinit::FacebookInit;
use futures::{future::ok, Future, IntoFuture};
use futures_ext::FutureExt;
use futures_preview::{future, prelude::*};
use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use hyper::{
    body::{Body, Payload},
    Response,
};
use scuba::ScubaSampleBuilder;
use slog::{o, Logger};
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::{
    self,
    sync::oneshot::{channel, Receiver, Sender},
};

use super::{ClientIdentity, Middleware};

use crate::config::ServerConfig;

type PostRequestCallback =
    Box<dyn FnOnce(&Duration, &Option<String>, Option<u64>, &PerfCounters) + Sync + Send + 'static>;

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
    pub headers_duration: Option<Duration>,
    pub should_log: bool,

    checkpoint: Option<Receiver<u64>>,
    start_time: Instant,
    post_request_callbacks: Vec<PostRequestCallback>,
}

impl RequestContext {
    fn new(ctx: CoreContext, should_log: bool) -> Self {
        Self {
            ctx,
            repository: None,
            method: None,
            error_msg: None,
            headers_duration: None,
            should_log,
            start_time: Instant::now(),
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

    pub fn headers_ready(&mut self) {
        self.headers_duration = Some(self.start_time.elapsed());
    }

    pub fn add_post_request<T>(&mut self, callback: T)
    where
        T: FnOnce(&Duration, &Option<String>, Option<u64>, &PerfCounters) + Sync + Send + 'static,
    {
        self.post_request_callbacks.push(Box::new(callback));
    }

    pub fn start_time(&self) -> Instant {
        self.start_time
    }

    /// Delay post request until a callback has completed. This is useful to e.g. record how much data was sent.
    pub fn delay_post_request(&mut self) -> Sender<u64> {
        // NOTE: If this is called twice ... then the first one will be ignored
        let (sender, receiver) = channel();
        self.checkpoint = Some(receiver);
        sender
    }

    fn dispatch_post_request(
        self,
        client_address: Option<IpAddr>,
        content_length: Option<u64>,
        disable_hostname_logging: bool,
    ) {
        let Self {
            ctx,
            start_time,
            post_request_callbacks,
            checkpoint,
            ..
        } = self;

        let run_callbacks =
            move |elapsed, client_hostname, bytes_sent, perf_counters: &PerfCounters| {
                for callback in post_request_callbacks.into_iter() {
                    callback(&elapsed, &client_hostname, bytes_sent, perf_counters)
                }
            };

        // We get the client hostname in post request, because that might be a little slow.
        let client_hostname = match disable_hostname_logging {
            true => ok(None).left_future(),
            _ => tokio_preview::task::spawn_blocking(move || -> Result<_, Error> {
                let hostname = client_address
                    .as_ref()
                    .map(lookup_addr)
                    .transpose()
                    .ok()
                    .flatten();

                Ok(hostname)
            })
            .map_err(|e| Error::new(e))
            .and_then(|r| future::ready(r))
            .compat()
            .or_else(|_| -> Result<_, !> { Ok(None) })
            .right_future(),
        };

        let fut = if let Some(checkpoint) = checkpoint {
            // NOTE: We use then() here: if the receiver was dropped, we still want to run our
            // callbacks!
            let request_complete =
                checkpoint
                    .into_future()
                    .then(move |bytes_sent| -> Result<_, !> {
                        Ok((start_time.elapsed(), bytes_sent.map(Some).unwrap_or(None)))
                    });

            (request_complete, client_hostname)
                .into_future()
                .map(move |((elapsed, bytes_sent), client_hostname)| {
                    run_callbacks(elapsed, client_hostname, bytes_sent, ctx.perf_counters());
                })
                .left_future()
        } else {
            let elapsed = start_time.elapsed();

            client_hostname
                .map(move |client_hostname| {
                    run_callbacks(
                        elapsed,
                        client_hostname,
                        content_length,
                        ctx.perf_counters(),
                    );
                })
                .right_future()
        };

        tokio::spawn(fut.discard());
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

impl Middleware for RequestContextMiddleware {
    fn inbound(&self, state: &mut State) {
        let request_id = request_id(&state);

        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(logger, ScubaSampleBuilder::with_discard());

        let should_log = ClientIdentity::try_borrow_from(&state)
            .map(|client_identity| !client_identity.is_proxygen_test_identity())
            .unwrap_or(true);

        state.put(RequestContext::new(ctx, should_log));
    }

    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        let client_address = ClientIdentity::try_borrow_from(&state)
            .map(|client_identity| *client_identity.address())
            .flatten();

        let content_length = response.body().content_length();

        let config = self.config_handle.get();
        if let Some(ctx) = state.try_take::<RequestContext>() {
            ctx.dispatch_post_request(
                client_address,
                content_length,
                config.disable_hostname_logging,
            );
        }
    }
}
