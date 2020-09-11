/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;
use gotham_ext::middleware::{ClientIdentity, Middleware, PostRequestCallbacks};
use hyper::StatusCode;
use hyper::{Body, Response};
use stats::prelude::*;

use crate::handlers::{EdenApiMethod, HandlerInfo};

define_stats! {
    prefix = "mononoke.edenapi.request";
    requests: dynamic_timeseries("{}.requests", (repo_and_method: String); Rate, Sum),
    success: dynamic_timeseries("{}.success", (repo_and_method: String); Rate, Sum),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); Rate, Sum),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); Rate, Sum),
    response_bytes_sent: dynamic_histogram("{}.response_bytes_sent", (repo_and_method: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    files_duration: dynamic_histogram("{}.files_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    trees_duration: dynamic_histogram("{}.trees_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    complete_trees_duration: dynamic_histogram("{}.complete_trees_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    history_duration: dynamic_histogram("{}.history_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    commit_location_to_hash_duration: dynamic_histogram("{}.commit_location_to_hash_ms", (repo: String); 10, 0, 500, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    commit_revlog_data_duration: dynamic_histogram("{}.commit_revlog_data_ms", (repo: String); 10, 0, 500, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    // Proxygen can be configured to periodically send a preconfigured set of
    // requests to check server health. These requests will look like ordinary
    // user requests, but should be filtered out of the server's metrics.
    match state.try_borrow::<ClientIdentity>() {
        Some(id) if id.is_proxygen_test_identity() => return None,
        _ => {}
    }

    let hander_info = state.try_borrow::<HandlerInfo>()?;
    let method = hander_info.method?;
    let repo = hander_info.repo.clone()?;
    let repo_and_method = format!("{}.{}", &repo, method.to_string());

    let callbacks = state.try_borrow_mut::<PostRequestCallbacks>()?;

    callbacks.add(move |info| {
        if let Some(duration) = info.duration {
            let dur_ms = duration.as_millis() as i64;

            use EdenApiMethod::*;
            match method {
                Files => STATS::files_duration.add_value(dur_ms, (repo,)),
                Trees => STATS::trees_duration.add_value(dur_ms, (repo,)),
                CompleteTrees => STATS::complete_trees_duration.add_value(dur_ms, (repo,)),
                History => STATS::history_duration.add_value(dur_ms, (repo,)),
                CommitLocationToHash => {
                    STATS::commit_location_to_hash_duration.add_value(dur_ms, (repo,))
                }
                CommitRevlogData => STATS::commit_revlog_data_duration.add_value(dur_ms, (repo,)),
            }
        }

        STATS::requests.add_value(1, (repo_and_method.clone(),));

        if status.is_success() {
            STATS::success.add_value(1, (repo_and_method.clone(),));
        } else if status.is_client_error() {
            STATS::failure_4xx.add_value(1, (repo_and_method.clone(),));
        } else if status.is_server_error() {
            STATS::failure_5xx.add_value(1, (repo_and_method.clone(),));
        }

        if let Some(response_bytes_sent) = info.bytes_sent {
            STATS::response_bytes_sent.add_value(response_bytes_sent as i64, (repo_and_method,))
        }
    });

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
