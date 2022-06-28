/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;

use gotham::state::State;
use hyper::Body;
use hyper::Response;

pub mod client_identity;
pub mod load;
pub mod log;
pub mod post_request;
pub mod scuba;
pub mod server_identity;
pub mod timer;
pub mod tls_session_data;

pub use self::client_identity::ClientIdentity;
pub use self::client_identity::ClientIdentityMiddleware;
pub use self::load::LoadMiddleware;
pub use self::load::RequestLoad;
pub use self::log::LogMiddleware;
pub use self::post_request::PostResponseCallbacks;
pub use self::post_request::PostResponseConfig;
pub use self::post_request::PostResponseInfo;
pub use self::post_request::PostResponseMiddleware;
pub use self::scuba::HttpScubaKey;
pub use self::scuba::ScubaHandler;
pub use self::scuba::ScubaMiddleware;
pub use self::scuba::ScubaMiddlewareState;
pub use self::server_identity::ServerIdentityMiddleware;
pub use self::timer::HeadersDuration;
pub use self::timer::RequestStartTime;
pub use self::timer::TimerMiddleware;
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
