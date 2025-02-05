/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::prelude::FromState;
use gotham::state::State;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::PostResponseCallbacks;
use gotham_ext::middleware::RequestLoad;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use permission_checker::MononokeIdentitySetExt;

#[cfg(fbcode_build)]
use crate::middleware::response::facebook::log_ods3;
#[cfg(not(fbcode_build))]
use crate::middleware::response::oss::log_ods3;
use crate::model::GitMethodInfo;

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    // Proxygen can be configured to periodically send a preconfigured set of
    // requests to check server health. These requests will look like ordinary
    // user requests, but should be filtered out of the server's metrics.
    match state.try_borrow::<MetadataState>() {
        Some(state) if state.metadata().identities().is_proxygen_test_identity() => {
            return None;
        }
        _ => {}
    }

    let method_info = state.try_borrow::<GitMethodInfo>()?;
    let method = method_info.method.clone();
    let method_variants = method_info.variants_to_string();
    let repo = method_info.repo.clone();
    let request_load = RequestLoad::try_borrow_from(state).map(|r| r.0 as f64);

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;

    callbacks.add(move |info| {
        let method = method.to_string();

        log_ods3(info, &status, method, method_variants, repo, request_load);
    });

    Some(())
}

pub struct Ods3Middleware;

impl Ods3Middleware {
    pub fn new() -> Self {
        Ods3Middleware
    }
}

#[async_trait::async_trait]
impl Middleware for Ods3Middleware {
    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        log_stats(state, response.status());
    }
}
