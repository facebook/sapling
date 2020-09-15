/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cached_config::ConfigHandle;
use futures::{
    channel::oneshot::{self, Receiver, Sender},
    prelude::*,
};
use gotham::state::{FromState, State};
use gotham_derive::StateData;
use hyper::{body::Body, Response};
use tokio::task;

use crate::response::ResponseContentMeta;

use super::{ClientIdentity, Middleware, RequestStartTime};

type Callback = Box<dyn FnOnce(&PostRequestInfo) + Send + 'static>;

/// Information passed to each post-request callback.
pub struct PostRequestInfo {
    pub duration: Option<Duration>,
    pub bytes_sent: Option<u64>,
    pub content_meta: Option<ResponseContentMeta>,
    pub client_hostname: Option<String>,
}

/// Trait allowing post-request callbacks to be configured dynamically.
pub trait PostRequestConfig: Clone + Send + Sync + RefUnwindSafe + 'static {
    /// Specify whether the middleware should perform a potentially
    /// expensive reverse DNS lookup of the client's hostname.
    fn resolve_hostname(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct DefaultConfig;

impl PostRequestConfig for DefaultConfig {}

impl<C: PostRequestConfig> PostRequestConfig for ConfigHandle<C> {
    fn resolve_hostname(&self) -> bool {
        self.get().resolve_hostname()
    }
}

/// Middleware that allows the application to register callbacks which will
/// be run upon request completion.
pub struct PostRequestMiddleware<C> {
    config: C,
}

impl<C> PostRequestMiddleware<C> {
    pub fn with_config(config: C) -> Self {
        Self { config }
    }
}

impl Default for PostRequestMiddleware<DefaultConfig> {
    fn default() -> Self {
        PostRequestMiddleware::with_config(DefaultConfig)
    }
}

#[async_trait]
impl<C: PostRequestConfig> Middleware for PostRequestMiddleware<C> {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        state.put(PostRequestCallbacks::new());
        None
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        let config = self.config.clone();
        let start_time = RequestStartTime::try_borrow_from(&state).map(|t| t.0);
        let content_meta = ResponseContentMeta::try_borrow_from(&state).copied();
        let hostname_future = ClientIdentity::try_borrow_from(&state).map(|id| id.hostname());

        if let Some(callbacks) = state.try_take::<PostRequestCallbacks>() {
            task::spawn(callbacks.run(config, start_time, content_meta, hostname_future));
        }
    }
}

/// A collection of callbacks that will run once the request has completed.
#[derive(StateData)]
pub struct PostRequestCallbacks {
    callbacks: Vec<Callback>,
    delay_signal: Option<Receiver<u64>>,
}

impl PostRequestCallbacks {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
            delay_signal: None,
        }
    }

    /// Add a callback that will be run once the request has completed. This is
    /// primarily useful for things like logging.
    ///
    /// Note that the callbacks are run serially in a task on the Tokio runtime.
    /// Although these callbacks are not asynchronous, they SHOULD NOT BLOCK as
    /// this could block the server from handling other requests.
    pub fn add<F>(&mut self, callback: F)
    where
        F: FnOnce(&PostRequestInfo) + Send + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }

    /// Delay execution of post-request callbacks until a value is sent over the
    /// given channel. This will typically be used in conjunction with something
    /// like `SignalStream` to delay execution until the entire request body has
    /// been sent. The value sent over the channel should be the number of bytes
    /// actually sent to the client (which may differ from the Content-Length).
    ///
    /// Note: If this method is called multiple times, only the channel from the
    /// most recent call will have any effect.
    pub fn delay(&mut self) -> Sender<u64> {
        let (sender, receiver) = oneshot::channel();
        self.delay_signal = Some(receiver);
        sender
    }

    async fn run<C, H>(
        self,
        config: C,
        start_time: Option<Instant>,
        content_meta: Option<ResponseContentMeta>,
        hostname_future: Option<H>,
    ) where
        C: PostRequestConfig,
        H: Future<Output = Option<String>> + Send + 'static,
    {
        let Self {
            callbacks,
            delay_signal,
        } = self;

        // If a delay has been set, wait until the entire response has been
        // sent before running the callbacks. If the delay channel returns
        // an error (meaning the sender was dropped), the callbacks will
        // still run, but the bytes sent will be reported as `None` as this
        // suggests that the response body may not have been fully sent.
        let bytes_sent = match delay_signal {
            Some(rx) => rx.await.ok(),
            None => content_meta.and_then(|m| m.content_length()),
        };

        // Capture elapsed time before waiting for the client hostname to resolve.
        let duration = start_time.map(|start| start.elapsed());

        // Resolve client hostname if enabled.
        let client_hostname = match hostname_future {
            Some(hostname) if config.resolve_hostname() => hostname.await,
            _ => None,
        };

        let info = PostRequestInfo {
            duration,
            bytes_sent,
            content_meta,
            client_hostname,
        };

        for callback in callbacks {
            callback(&info);
        }
    }
}
