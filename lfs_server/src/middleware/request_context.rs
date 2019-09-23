// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use futures::{Future, IntoFuture};
use gotham::state::State;
use gotham_derive::StateData;
use hyper::{Body, Response};
use std::time::{Duration, Instant};
use tokio::{
    self,
    sync::oneshot::{channel, Receiver, Sender},
};

use super::Middleware;

type PostRequestCallback = Box<dyn FnOnce(&Duration) + Sync + Send + 'static>;

#[derive(Copy, Clone)]
pub enum LfsMethod {
    Upload,
    Download,
    Batch,
}

impl fmt::Display for LfsMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Batch => "batch",
        };
        write!(f, "{}", name)
    }
}

#[derive(StateData)]
pub struct RequestContext {
    pub repository: Option<String>,
    pub method: Option<LfsMethod>,
    pub error_msg: Option<String>,
    pub response_size: Option<u64>,
    pub headers_duration: Option<Duration>,

    checkpoint: Option<Receiver<()>>,
    start_time: Instant,
    post_request_callbacks: Vec<PostRequestCallback>,
}

impl RequestContext {
    fn new() -> Self {
        Self {
            repository: None,
            method: None,
            error_msg: None,
            response_size: None,
            headers_duration: None,
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

    pub fn set_response_size(&mut self, size: u64) {
        self.response_size = Some(size);
    }

    pub fn headers_ready(&mut self) {
        self.headers_duration = Some(self.start_time.elapsed());
    }

    pub fn add_post_request<T>(&mut self, callback: T)
    where
        T: FnOnce(&Duration) + Sync + Send + 'static,
    {
        self.post_request_callbacks.push(Box::new(callback));
    }

    pub fn delay_post_request(&mut self) -> Sender<()> {
        // NOTE: If this is called twice ... then the first one will be ignored
        let (sender, receiver) = channel();
        self.checkpoint = Some(receiver);
        sender
    }

    fn dispatch_post_request(self) {
        let Self {
            start_time,
            post_request_callbacks,
            checkpoint,
            ..
        } = self;

        let run_callbacks = move || {
            let elapsed = start_time.elapsed();
            for callback in post_request_callbacks.into_iter() {
                callback(&elapsed)
            }
        };

        if let Some(checkpoint) = checkpoint {
            // NOTE: We use then() here: if the receiver was dropped, we still want to run our
            // callbacks! In fact, right now, for reasons unknown but probably having to do with
            // content length our data streams never get polled to completion.
            let fut = checkpoint.into_future().then(move |_| {
                run_callbacks();
                Ok(())
            });

            tokio::spawn(fut);
        } else {
            run_callbacks();
        }
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {}

impl RequestContextMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

impl Middleware for RequestContextMiddleware {
    fn inbound(&self, state: &mut State) {
        state.put(RequestContext::new());
    }

    fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
        if let Some(ctx) = state.try_take::<RequestContext>() {
            ctx.dispatch_post_request();
        }
    }
}
