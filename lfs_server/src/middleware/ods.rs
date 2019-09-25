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

use super::{LfsMethod, Middleware, RequestContext};

define_stats! {
    prefix = "mononoke.lfs.request";
    requests: dynamic_timeseries("{}.requests", (repo_and_method: String); RATE, SUM),
    success: dynamic_timeseries("{}.success", (repo_and_method: String); RATE, SUM),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); RATE, SUM),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); RATE, SUM),
    upload_duration: dynamic_histogram("{}.upload_ms", (repo: String); 100, 0, 5000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    download_duration: dynamic_histogram("{}.download_ms", (repo: String); 100, 0, 5000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    batch_duration: dynamic_histogram("{}.batch_ms", (repo: String); 10, 0, 500, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    let ctx = state.try_borrow_mut::<RequestContext>()?;
    let method = ctx.method?;
    let repo_and_method = format!("{}.{}", ctx.repository.as_ref()?, method.to_string());

    ctx.add_post_request(move |duration, _| {
        match method {
            LfsMethod::Upload => STATS::upload_duration
                .add_value(duration.as_millis_unchecked() as i64, (method.to_string(),)),
            LfsMethod::Download => STATS::download_duration
                .add_value(duration.as_millis_unchecked() as i64, (method.to_string(),)),
            LfsMethod::Batch => STATS::batch_duration
                .add_value(duration.as_millis_unchecked() as i64, (method.to_string(),)),
        }

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
