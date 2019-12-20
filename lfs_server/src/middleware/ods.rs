/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use gotham::state::State;
use hyper::StatusCode;
use hyper::{Body, Response};
use stats::prelude::*;
use time_ext::DurationExt;

use super::{LfsMethod, Middleware, RequestContext};

define_stats! {
    prefix = "mononoke.lfs.request";
    requests: dynamic_timeseries("{}.requests", (repo_and_method: String); Rate, Sum),
    success: dynamic_timeseries("{}.success", (repo_and_method: String); Rate, Sum),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); Rate, Sum),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); Rate, Sum),
    upload_duration: dynamic_histogram("{}.upload_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    download_duration: dynamic_histogram("{}.download_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    download_sha256_duration: dynamic_histogram("{}.download_sha256_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    batch_duration: dynamic_histogram("{}.batch_ms", (repo: String); 10, 0, 500, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    response_bytes_sent: dynamic_histogram("{}.response_bytes_sent", (repo_and_method: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    let ctx = state.try_borrow_mut::<RequestContext>()?;
    let method = ctx.method?;
    let repo = ctx.repository.clone()?;
    let repo_and_method = format!("{}.{}", &repo, method.to_string());

    if !ctx.should_log {
        return None;
    }

    ctx.add_post_request(move |duration, _, response_bytes_sent, _| {
        match method {
            LfsMethod::Upload => {
                STATS::upload_duration.add_value(duration.as_millis_unchecked() as i64, (repo,))
            }
            LfsMethod::Download => {
                STATS::download_duration.add_value(duration.as_millis_unchecked() as i64, (repo,))
            }
            LfsMethod::DownloadSha256 => STATS::download_sha256_duration
                .add_value(duration.as_millis_unchecked() as i64, (repo,)),
            LfsMethod::Batch => {
                STATS::batch_duration.add_value(duration.as_millis_unchecked() as i64, (repo,))
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

        if let Some(response_bytes_sent) = response_bytes_sent {
            STATS::response_bytes_sent
                .add_value(response_bytes_sent as i64, (repo_and_method.clone(),))
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
