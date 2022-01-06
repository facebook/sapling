/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;

use gotham::state::State;
use hyper::{Body, Response};

pub mod client_identity;
pub mod load;
pub mod log;
pub mod post_request;
pub mod scuba;
pub mod server_identity;
pub mod timer;
pub mod tls_session_data;

pub use self::client_identity::{ClientIdentity, ClientIdentityMiddleware};
pub use self::load::{LoadMiddleware, RequestLoad};
pub use self::log::LogMiddleware;
pub use self::post_request::{
    PostResponseCallbacks, PostResponseConfig, PostResponseInfo, PostResponseMiddleware,
};
pub use self::scuba::{HttpScubaKey, ScubaHandler, ScubaMiddleware, ScubaMiddlewareState};
pub use self::server_identity::ServerIdentityMiddleware;
pub use self::timer::{HeadersDuration, RequestStartTime, TimerMiddleware};
pub use self::tls_session_data::TlsSessionDataMiddleware;

#[async_trait::async_trait]
pub trait Middleware: 'static + RefUnwindSafe + Send + Sync {
    async fn inbound(&self, _state: &mut State) -> Option<Response<Body>> {
        // Implement inbound to perform pre-request actions, such as putting something in the
        // state.

        None
    }

    async fn outbound(&self, _state: &mut State, _response: &mut Response<Body>) {
        // Implement outbound to perform post-request actions, such as logging the response status
        // code.
    }
}
