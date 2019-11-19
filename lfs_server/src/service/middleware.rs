/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use fbinit::FacebookInit;
use futures::future::Future;
use futures_ext::FutureExt;
use gotham::{handler::HandlerFuture, middleware::Middleware, state::State};
use gotham_derive::NewMiddleware;
use stats::service_data::{get_service_data_singleton, ServiceData, ServiceDataWrapper};
use std::convert::TryInto;
use std::time::Duration;

use crate::config::ServerConfigHandle;

use crate::errors::ErrorKind;
use crate::http::HttpError;

use super::util::http_error_to_handler_error;

// NOTE: Our Throttling middleware is implemented as Gotham middleware for 3 reasons:
// - It needs to replace responses.
// - It needs to do asynchronously.
// - It only needs to run if we're going to serve a request.

#[derive(Clone, NewMiddleware)]
pub struct ThrottleMiddleware {
    fb: FacebookInit,
    handle: ServerConfigHandle,
}

impl ThrottleMiddleware {
    pub fn new(fb: FacebookInit, handle: ServerConfigHandle) -> Self {
        Self { fb, handle }
    }
}

impl Middleware for ThrottleMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        let service_data = get_service_data_singleton(self.fb);

        for limit in self.handle.get().throttle_limits.iter() {
            if let Some(err) = is_limit_exceeded(&service_data, &limit.counter, limit.limit) {
                let err = HttpError::e429(err);
                let sleep_ms: u64 = limit.sleep_ms.try_into().unwrap_or(0);
                return tokio_timer::sleep(Duration::from_millis(sleep_ms))
                    .then(move |_| http_error_to_handler_error(err, state))
                    .boxify();
            }
        }

        chain(state).boxify()
    }
}

fn is_limit_exceeded(
    service_data: &ServiceDataWrapper,
    key: &str,
    limit: i64,
) -> Option<ErrorKind> {
    // NOTE: This checks local limits for this individual process by looking at fb303 counters.
    match service_data.get_counter(&key) {
        Some(value) if value > limit => Some(ErrorKind::Throttled(key.to_string(), value, limit)),
        _ => None,
    }
}
