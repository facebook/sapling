/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;

use dns_lookup::lookup_addr;
use failure::Error;
use futures::{Future, IntoFuture};
use futures_ext::{asynchronize, FutureExt};
use gotham::state::{FromState, State};
use gotham_derive::StateData;
use hyper::{Body, Response};
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::{
    self,
    sync::oneshot::{channel, Receiver, Sender},
};

use super::{ClientIdentity, Middleware};

type PostRequestCallback = Box<dyn FnOnce(&Duration, &Option<String>) + Sync + Send + 'static>;

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
        T: FnOnce(&Duration, &Option<String>) + Sync + Send + 'static,
    {
        self.post_request_callbacks.push(Box::new(callback));
    }

    pub fn delay_post_request(&mut self) -> Sender<()> {
        // NOTE: If this is called twice ... then the first one will be ignored
        let (sender, receiver) = channel();
        self.checkpoint = Some(receiver);
        sender
    }

    fn dispatch_post_request(self, client_address: Option<IpAddr>) {
        let Self {
            start_time,
            post_request_callbacks,
            checkpoint,
            ..
        } = self;

        let run_callbacks = move |elapsed, client_hostname| {
            for callback in post_request_callbacks.into_iter() {
                callback(&elapsed, &client_hostname)
            }
        };

        // We get the client hostname in post request, because that might be a little slow.
        let client_hostname = asynchronize(move || -> Result<_, Error> {
            let hostname = client_address
                .as_ref()
                .map(lookup_addr)
                .transpose()
                .ok()
                .flatten();

            Ok(hostname)
        })
        .or_else(|_| -> Result<_, !> { Ok(None) });

        let fut = if let Some(checkpoint) = checkpoint {
            // NOTE: We use then() here: if the receiver was dropped, we still want to run our
            // callbacks! In fact, right now, for reasons unknown but probably having to do with
            // content length our data streams never get polled to completion.
            let request_complete = checkpoint
                .into_future()
                .then(move |_| -> Result<_, !> { Ok(start_time.elapsed()) });

            (request_complete, client_hostname)
                .into_future()
                .map(move |(elapsed, client_hostname)| {
                    run_callbacks(elapsed, client_hostname);
                })
                .left_future()
        } else {
            let elapsed = start_time.elapsed();

            client_hostname
                .map(move |client_hostname| {
                    run_callbacks(elapsed, client_hostname);
                })
                .right_future()
        };

        tokio::spawn(fut.discard());
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
        let client_address = ClientIdentity::try_borrow_from(&state)
            .map(|client_identity| *client_identity.address())
            .flatten();

        if let Some(ctx) = state.try_take::<RequestContext>() {
            ctx.dispatch_post_request(client_address);
        }
    }
}
