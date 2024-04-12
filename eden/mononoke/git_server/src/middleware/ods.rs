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
use stats::prelude::*;

use crate::model::GitMethod;
use crate::model::GitMethodInfo;

define_stats! {
    prefix = "mononoke.git.request";
    request_load: histogram(100, 0, 5000, Average; P 50; P 75; P 95; P 99),
    requests: dynamic_timeseries("{}.requests", (method: String); Rate, Sum),
    success: dynamic_timeseries("{}.success", (method: String); Rate, Sum),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (method: String); Rate, Sum),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (method: String); Rate, Sum),
    response_bytes_sent: dynamic_histogram("{}.response_bytes_sent", (method: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    clone_duration_ms: dynamic_histogram("{}.clone_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    pull_duration_ms: dynamic_histogram("{}.pull_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    ls_refs_duration_ms: dynamic_histogram("{}.ls_refs_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

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
    let repo = method_info.repo.clone();

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;

    callbacks.add(move |info| {
        if let Some(duration) = info.duration {
            let dur_ms = duration.as_millis() as i64;

            use GitMethod::*;
            match method {
                Pull => STATS::pull_duration_ms.add_value(dur_ms, (repo.clone(),)),
                Clone => STATS::clone_duration_ms.add_value(dur_ms, (repo.clone(),)),
                LsRefs => STATS::ls_refs_duration_ms.add_value(dur_ms, (repo.clone(),)),
            }
        }

        let method = method.to_string();
        STATS::requests.add_value(1, (method.clone(),));

        if status.is_success() {
            STATS::success.add_value(1, (method.clone(),));
        } else if status.is_client_error() {
            STATS::failure_4xx.add_value(1, (method.clone(),));
        } else if status.is_server_error() {
            STATS::failure_5xx.add_value(1, (method.clone(),));
        }

        if let Some(response_bytes_sent) = info.meta.as_ref().map(|m| m.body().bytes_sent) {
            STATS::response_bytes_sent.add_value(response_bytes_sent as i64, (method,))
        }
    });

    if let Some(request_load) = RequestLoad::try_borrow_from(state) {
        STATS::request_load.add_value(request_load.0);
    }

    Some(())
}

pub struct OdsMiddleware;

impl OdsMiddleware {
    pub fn new() -> Self {
        OdsMiddleware
    }
}

#[async_trait::async_trait]
impl Middleware for OdsMiddleware {
    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        log_stats(state, response.status());
    }
}
