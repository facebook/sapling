// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::State;
use hyper::{Body, Response};
use std::panic::RefUnwindSafe;

mod identity;
mod log;
mod ods;
mod request_context;
mod scuba;
mod timer;

pub use self::identity::IdentityMiddleware;
pub use self::log::LogMiddleware;
pub use self::ods::OdsMiddleware;
pub use self::request_context::{RequestContext, RequestContextMiddleware};
pub use self::scuba::{ScubaMiddleware, ScubaMiddlewareState};
pub use self::timer::TimerMiddleware;

pub type Callback = Box<dyn FnOnce(&mut State, &mut Response<Body>) + 'static + Send + Sync>;

pub trait Middleware: 'static + RefUnwindSafe + Send + Sync {
    fn handle(&self, state: &mut State) -> Callback;
}
