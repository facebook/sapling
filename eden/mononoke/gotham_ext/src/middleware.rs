/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;

use gotham::state::State;
use hyper::{Body, Response};

pub mod server_identity;

pub use server_identity::ServerIdentityMiddleware;

pub trait Middleware: 'static + RefUnwindSafe + Send + Sync {
    fn inbound(&self, _state: &mut State) {
        // Implement inbound to perform pre-request actions, such as putting something in the
        // state.
    }

    fn outbound(&self, _state: &mut State, _response: &mut Response<Body>) {
        // Implement outbound to perform post-request actions, such as logging the response status
        // code.
    }
}
