// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::State;
use gotham_derive::StateData;
use std::time::Duration;

use super::{Callback, Middleware};

#[derive(Clone, StateData, Default)]
pub struct RequestContext {
    pub repository: Option<String>,
    pub method: Option<&'static str>,
    pub error_msg: Option<String>,
    pub response_size: Option<u64>,
    pub duration: Option<Duration>,
}

impl RequestContext {
    pub fn set_request(&mut self, repository: String, method: &'static str) {
        self.repository = Some(repository);
        self.method = Some(method);
    }

    pub fn set_error_msg(&mut self, error_msg: String) {
        self.error_msg = Some(error_msg);
    }

    pub fn set_response_size(&mut self, size: u64) {
        self.response_size = Some(size);
    }

    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = Some(duration);
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
    fn handle(&self, state: &mut State) -> Callback {
        state.put(RequestContext::default());
        Box::new(|_state, _response| {})
    }
}
