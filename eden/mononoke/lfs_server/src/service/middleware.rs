/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use gotham::{
    handler::HandlerFuture,
    middleware::Middleware,
    state::{FromState, State},
};
use gotham_derive::NewMiddleware;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::ClientIdentity;
use hyper::Uri;
use permission_checker::MononokeIdentitySet;
use rand::Rng;
use stats::prelude::*;
use std::convert::TryInto;
use std::pin::Pin;
use std::time::Duration;

use crate::config::{Limit, ServerConfig};
use crate::errors::ErrorKind;

use super::util::http_error_to_handler_error;

define_stats! {
    // We use a dynamic singleton counter here instead of direct usage of FB API to make use of
    // the OSS friendly API.
    throttle_counter: dynamic_singleton_counter("{}", (key: String)),
}

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

        for limit in self.handle.get().throttle_limits().iter() {
            if !limit_applies_to_client(&limit, &identities) {
                continue;
            }

            if !limit_applies_probabilistically(&limit) {
                continue;
            }

            if let Some(err) = is_limit_exceeded(self.fb, &limit.counter(), limit.limit()) {
                let err = HttpError::e429(err);

                let sleep_ms: u64 = limit.sleep_ms().try_into().unwrap_or(0);
                let max_jitter_ms: u64 = limit.max_jitter_ms().try_into().unwrap_or(0);
                let mut jitter: u64 = 0;

                if max_jitter_ms > 0 {
                    jitter = rand::thread_rng().gen_range(0, max_jitter_ms);
                }

                let total_sleep_ms = sleep_ms + jitter;

                let res = async move {
                    if total_sleep_ms > 0 {
                        tokio::time::delay_for(Duration::from_millis(total_sleep_ms)).await;
                    }
                    http_error_to_handler_error(err, state)
                }
                .boxed();

                return res;
            }
        }

        chain(state)
    }
}

fn is_limit_exceeded(fb: FacebookInit, key: &str, limit: i64) -> Option<ErrorKind> {
    // NOTE: This checks local limits for this individual process by looking at fb303 counters.
    match STATS::throttle_counter.get_value(fb, (key.to_owned(),)) {
        Some(value) if value > limit => Some(ErrorKind::Throttled(key.to_string(), value, limit)),
        _ => None,
    }
}

fn limit_applies_to_client(limit: &Limit, client_identity: &Option<&MononokeIdentitySet>) -> bool {
    let client_identity_sets = limit.client_identity_sets();
    let configured_identities = match client_identity_sets.is_empty() {
        true => return true,
        false => client_identity_sets,
    };

    let presented_identities = match client_identity {
        Some(value) => value,
        _ => return false,
    };

    configured_identities
        .iter()
        .any(|limit_ids| limit_ids.is_subset(&presented_identities))
}

fn limit_applies_probabilistically(limit: &Limit) -> bool {
    limit.probability_pct() > rand::thread_rng().gen_range(0, 100)
}

#[cfg(test)]
mod test {
    use super::*;
    use permission_checker::MononokeIdentity;
    use std::collections::BTreeSet;

    #[test]
    fn test_limit_applies_to_client() {
        let limit_config = lfs_server_config::ThrottleLimit {
            counter: "".to_string(),
            limit: 0,
            sleep_ms: 0,
            max_jitter_ms: 0,
            client_identities: vec![],
            client_identity_sets: vec![vec![
                "MACHINE_TIER:od".to_string(),
                "SERVICE_IDENTITY:bambuko".to_string(),
            ]],
            probability_pct: 0,
        };

        let limit: Limit = limit_config.try_into().unwrap();

        let mut identities = BTreeSet::new();
        let svc_ident = MononokeIdentity::new("SERVICE_IDENTITY", "bambuko").unwrap();
        identities.insert(MononokeIdentity::new("MACHINE_TIER", "od").unwrap());
        identities.insert(svc_ident.clone());
        identities.insert(MononokeIdentity::new("MACHINE", "od123.ftw").unwrap());

        assert!(limit_applies_to_client(&limit, &Some(&identities)));
        identities.remove(&svc_ident);
        assert!(!limit_applies_to_client(&limit, &Some(&identities)));
    }

    #[test]
    fn test_limit_applies_probabilistically() {
        let base = lfs_server_config::ThrottleLimit {
            counter: "".to_string(),
            limit: 0,
            sleep_ms: 0,
            max_jitter_ms: 0,
            client_identities: vec![],
            client_identity_sets: vec![],
            probability_pct: 0,
        };

        let l0: Limit = lfs_server_config::ThrottleLimit {
            probability_pct: 0,
            ..base.clone()
        }
        .try_into()
        .unwrap();

        assert!(!limit_applies_probabilistically(&l0));

        let l100: Limit = lfs_server_config::ThrottleLimit {
            probability_pct: 100,
            ..base.clone()
        }
        .try_into()
        .unwrap();

        assert!(limit_applies_probabilistically(&l100));
    }
}
