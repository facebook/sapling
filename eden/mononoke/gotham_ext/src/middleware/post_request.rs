/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;
use std::time::Duration;
use std::time::Instant;

use anyhow::Error;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use futures::Future;
use futures::future;
use futures::prelude::*;
use futures_stats::StreamStats;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use hyper::Response;
use hyper::body::Body;
use mononoke_macros::mononoke;
use permission_checker::MononokeIdentitySetExt;
use trust_dns_resolver::TokioAsyncResolver;

use super::MetadataState;
use super::Middleware;
use super::RequestStartTime;
use crate::response::InBandErrors;
use crate::response::PendingInBandErrors;
use crate::response::PendingResponseMeta;
use crate::response::PendingStreamStats;
use crate::response::ResponseMeta;

type Callback = Box<dyn FnOnce(&PostResponseInfo) + Send + 'static>;

/// Information passed to each post-request callback.
pub struct PostResponseInfo {
    pub duration: Option<Duration>,
    pub client_hostname: Option<String>,
    pub meta: Option<ResponseMeta>,
    pub stream_stats: Option<StreamStats>,
    pub in_band_errors: Option<InBandErrors>,
}

impl PostResponseInfo {
    pub fn first_error(&self) -> Option<&Error> {
        if let Some(err) = self.meta.as_ref()?.body().error_meta.errors.first() {
            Some(err)
        } else if let Some(err) = self.in_band_errors.as_ref()?.errors.first() {
            Some(err)
        } else {
            None
        }
    }

    pub fn error_count(&self) -> u64 {
        let errors_count = self
            .meta
            .as_ref()
            .map_or(0, |m| m.body().error_meta.error_count());
        let in_band_errors_count = self
            .in_band_errors
            .as_ref()
            .map_or(0, |e| e.errors.len() as u64);
        errors_count + in_band_errors_count
    }
}

/// Trait allowing post-request callbacks to be configured dynamically.
pub trait PostResponseConfig: Clone + Send + Sync + RefUnwindSafe + 'static {
    /// Specify whether the middleware should perform a potentially
    /// expensive reverse DNS lookup of the client's hostname.
    fn resolve_hostname(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct DefaultConfig;

impl PostResponseConfig for DefaultConfig {}

impl<C: PostResponseConfig> PostResponseConfig for ConfigHandle<C> {
    fn resolve_hostname(&self) -> bool {
        self.get().resolve_hostname()
    }
}

/// Middleware that allows the application to register callbacks which will
/// be run upon request completion.
pub struct PostResponseMiddleware<C> {
    config: C,
}

impl<C> PostResponseMiddleware<C> {
    pub fn with_config(config: C) -> Self {
        Self { config }
    }
}

impl Default for PostResponseMiddleware<DefaultConfig> {
    fn default() -> Self {
        PostResponseMiddleware::with_config(DefaultConfig)
    }
}

#[async_trait]
impl<C: PostResponseConfig> Middleware for PostResponseMiddleware<C> {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        state.put(PostResponseCallbacks::new());
        None
    }

    async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        let config = self.config.clone();
        let start_time = RequestStartTime::try_borrow_from(state).map(|t| t.0);
        let hostname_future = MetadataState::try_borrow_from(state).map(resolve_hostname);
        let meta = PendingResponseMeta::try_take_from(state);
        let stream_stats = PendingStreamStats::try_take_from(state);
        let pending_in_band_errors = PendingInBandErrors::try_take_from(state);

        if let Some(callbacks) = state.try_take::<PostResponseCallbacks>() {
            mononoke::spawn_task(callbacks.run(
                config,
                start_time,
                meta,
                stream_stats,
                hostname_future,
                pending_in_band_errors,
            ));
        }
    }
}

/// A collection of callbacks that will run once the request has completed.
#[derive(StateData)]
pub struct PostResponseCallbacks {
    callbacks: Vec<Callback>,
}

impl PostResponseCallbacks {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
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
        F: FnOnce(&PostResponseInfo) + Send + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }

    async fn run<C, H>(
        self,
        config: C,
        start_time: Option<Instant>,
        meta: Option<PendingResponseMeta>,
        stream_stats: Option<PendingStreamStats>,
        hostname_future: Option<H>,
        pending_in_band_errors: Option<PendingInBandErrors>,
    ) where
        C: PostResponseConfig,
        H: Future<Output = Option<String>> + Send + 'static,
    {
        let Self { callbacks } = self;

        let meta = match meta {
            Some(meta) => Some(meta.finish().await),
            None => None,
        };

        let stream_stats = match stream_stats {
            Some(stream_stats) => stream_stats.finish().await,
            None => None,
        };

        // Capture elapsed time before waiting for the client hostname to resolve.
        let duration = start_time.map(|start| start.elapsed());

        // Resolve client hostname if enabled.
        let client_hostname = match hostname_future {
            Some(hostname) if config.resolve_hostname() => hostname.await,
            _ => None,
        };

        let in_band_errors = match pending_in_band_errors {
            Some(mut pending_in_band_errors) => Some(pending_in_band_errors.finish().await),
            None => None,
        };

        let info = PostResponseInfo {
            duration,
            client_hostname,
            meta,
            stream_stats,
            in_band_errors,
        };

        for callback in callbacks {
            callback(&info);
        }
    }
}

// Hostname of the client is for non-critical use only (best-effort lookup):
pub fn resolve_hostname(
    metadata_state: &MetadataState,
) -> impl Future<Output = Option<String>> + 'static + use<> {
    // XXX: Can't make this an async fn because the resulting Future would
    // have a non-'static lifetime (due to the &ClientIdentity argument).

    let metadata = metadata_state.metadata();
    // 1) We're extracting it from identities (which requires no remote calls)
    if let Some(client_hostname) = metadata.identities().hostname().map(|h| h.to_string()) {
        return future::ready(Some(client_hostname)).left_future();
    }
    // 2) Perform a reverse DNS lookup of the client's IP address to determine
    // its hostname.
    let address = metadata.client_ip().cloned();
    (async move {
        let resolver = TokioAsyncResolver::tokio_from_system_conf().ok()?;
        let hosts = resolver.reverse_lookup(address?).await.ok()?;
        let host = hosts.iter().next()?;
        Some(host.to_string().trim_end_matches('.').to_string())
    })
    .right_future()
}
