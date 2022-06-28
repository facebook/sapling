/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::NewMiddleware;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::ClientIdentity;
use gotham_ext::response::build_error_response;
use hyper::Uri;
use std::pin::Pin;

use crate::config::ServerConfig;

use super::error_formatter::LfsErrorFormatter;

// NOTE: Our Throttling middleware is implemented as Gotham middleware for 3 reasons:
// - It needs to replace responses.
// - It needs to do asynchronously.
// - It only needs to run if we're going to serve a request.

#[derive(Clone, NewMiddleware)]
pub struct ThrottleMiddleware {
    fb: FacebookInit,
    handle: ConfigHandle<ServerConfig>,
}

impl ThrottleMiddleware {
    pub fn new(fb: FacebookInit, handle: ConfigHandle<ServerConfig>) -> Self {
        Self { fb, handle }
    }
}

impl Middleware for ThrottleMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Pin<Box<HandlerFuture>>
    where
        Chain: FnOnce(State) -> Pin<Box<HandlerFuture>>,
    {
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return chain(state);
            }
        }

        let identities = if let Some(client_ident) = state.try_borrow::<ClientIdentity>() {
            client_ident.identities().as_ref()
        } else {
            None
        };

        for limit in self.handle.get().loadshedding_limits().iter() {
            if let Err(err) = limit.should_load_shed(self.fb, identities) {
                let err = HttpError::e429(err);

                let res =
                    async move { build_error_response(err, state, &LfsErrorFormatter) }.boxed();

                return res;
            }
        }

        chain(state)
    }
}
