// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use gotham::state::State;
use hyper::StatusCode;
use hyper::{Body, Response};
use stats::{define_stats, DynamicHistogram, DynamicTimeseries};
use time_ext::DurationExt;

use super::{Middleware, RequestContext};

define_stats! {
    prefix = "mononoke.lfs.request";
    requests: dynamic_timeseries("{}.requests", (repo_and_method: String); RATE, SUM),
    success: dynamic_timeseries("{}.success", (repo_and_method: String); RATE, SUM),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); RATE, SUM),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); RATE, SUM),
    duration: dynamic_histogram("{}_ms", (repo_and_method: String); 100, 0, 5000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    let ctx = state.try_borrow_mut::<RequestContext>()?;
    let repo_and_method = format!("{}.{}", ctx.repository.as_ref()?, ctx.method?);

    ctx.add_post_request(move |duration| {
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
    });

    Some(())
}

pub struct OdsMiddleware {}

impl OdsMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

impl Middleware for OdsMiddleware {
    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        log_stats(state, response.status());
    }
}
