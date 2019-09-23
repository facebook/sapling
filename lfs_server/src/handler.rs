// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::Future;
use gotham::{
    handler::{Handler, HandlerFuture, IntoResponse, NewHandler},
    router::Router,
    state::State,
};
use std::sync::Arc;

use crate::middleware::Middleware;

#[derive(Clone)]
pub struct MononokeLfsHandler {
    router: Router,
    middleware: Arc<Vec<Box<dyn Middleware>>>,
}

// This trait is boilerplate to let us bind this in Gotham as the service. This is essentially
// patterned match off of how the Router Handler works in Gotham.
impl NewHandler for MononokeLfsHandler {
    type Instance = MononokeLfsHandler;

    fn new_handler(&self) -> Result<Self::Instance, Error> {
        Ok(self.clone())
    }
}

// Gotham's router only runs middleware on route matches, which means we don't get any visibility
// into e.g. 404's. So, we use this handler to wrap Gotham's router and run our own middleware
// stack. Our middleware is also fairly minimal in the sense that it doesn't need to replace
// responses or prevent requests, so we expose a slightly simpler API.
impl Handler for MononokeLfsHandler {
    fn handle(self, mut state: State) -> Box<HandlerFuture> {
        // On request, middleware is called in order, then called the other way around on response.
        // This is what regular Router middleware in Gotham would do.
        let middleware = self.middleware.clone();
        for m in middleware.iter() {
            m.inbound(&mut state);
        }

        // NOTE: It's a bit unfortunate that we have to return a HandlerFuture here when really
        // we'd rather be working with just (State, HttpResponse<Body>) everywhere, but that's how
        // it is on Gotham.
        let fut = self.router.handle(state).then(move |res| {
            let (mut state, mut response) = res.unwrap_or_else(|(state, err)| {
                let response = err.into_response(&state);
                (state, response)
            });

            for m in middleware.iter().rev() {
                m.outbound(&mut state, &mut response);
            }

            Ok((state, response))
        });

        Box::new(fut)
    }
}

impl MononokeLfsHandler {
    pub fn builder() -> MononokeLfsHandlerBuilder {
        MononokeLfsHandlerBuilder { middleware: vec![] }
    }
}

pub struct MononokeLfsHandlerBuilder {
    middleware: Vec<Box<dyn Middleware>>,
}

impl MononokeLfsHandlerBuilder {
    pub fn add<T: Middleware>(mut self, middleware: T) -> Self {
        self.middleware.push(Box::new(middleware));
        self
    }

    pub fn build(self, router: Router) -> MononokeLfsHandler {
        MononokeLfsHandler {
            router,
            middleware: Arc::new(self.middleware),
        }
    }
}
