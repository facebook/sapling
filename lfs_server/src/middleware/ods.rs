// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Future;
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::state::State;
use gotham_derive::NewMiddleware;
use hyper::StatusCode;
use stats::{define_stats, DynamicHistogram, DynamicTimeseries};
use time_ext::DurationExt;

use crate::lfs_server_context::LoggingContext;

define_stats! {
    prefix = "mononoke.lfs.request";
    requests: dynamic_timeseries("{}.requests", (repo_and_method: String); RATE, SUM),
    success: dynamic_timeseries("{}.success", (repo_and_method: String); RATE, SUM),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); RATE, SUM),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); RATE, SUM),
    duration: dynamic_histogram("{}_ms", (repo_and_method: String); 100, 0, 5000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

#[derive(Clone, NewMiddleware)]
pub struct OdsMiddleware {}

impl OdsMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

fn log_stats(state: &State, status: &StatusCode) -> Option<()> {
    let log_ctx = state.try_borrow::<LoggingContext>()?;
    let duration = log_ctx.duration?;

    let repo_and_method = format!("{}.{}", log_ctx.repository, log_ctx.method);

    STATS::duration.add_value(
        duration.as_millis_unchecked() as i64,
        (repo_and_method.clone(),),
    );

    STATS::requests.add_value(1, (repo_and_method.clone(),));

    if status.is_success() {
        STATS::success.add_value(1, (repo_and_method.clone(),));
    } else if status.is_client_error() {
        STATS::failure_4xx.add_value(1, (repo_and_method.clone(),));
    } else if status.is_server_error() {
        STATS::failure_5xx.add_value(1, (repo_and_method.clone(),));
    }

    Some(())
}

impl Middleware for OdsMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        Box::new(chain(state).then(|res| {
            match res {
                Ok((ref state, ref response)) => {
                    log_stats(state, &response.status());
                }
                Err((ref state, _)) => {
                    log_stats(state, &StatusCode::INTERNAL_SERVER_ERROR);
                }
            }

            res
        }))
    }
}
