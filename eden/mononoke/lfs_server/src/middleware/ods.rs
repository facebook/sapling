/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::PostResponseCallbacks;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use stats::prelude::*;
use time_ext::DurationExt;

use super::LfsMethod;
use super::RequestContext;

define_stats! {
    prefix = "mononoke.lfs.request";
    success: timeseries(Rate, Sum),
    requests: timeseries(Rate, Sum),
    failure_4xx: timeseries(Rate, Sum),
    failure_429: timeseries(Rate, Sum),
    failure_404: timeseries(Rate, Sum),
    failure_5xx: timeseries(Rate, Sum),

    repo_requests: dynamic_timeseries("{}.requests", (repo_and_method: String); Rate, Sum),
    repo_success: dynamic_timeseries("{}.success", (repo_and_method: String); Rate, Sum),
    repo_failure_4xx: dynamic_timeseries("{}.failure_4xx", (repo_and_method: String); Rate, Sum),
    repo_failure_5xx: dynamic_timeseries("{}.failure_5xx", (repo_and_method: String); Rate, Sum),
    git_upload_blob_duration: dynamic_histogram("{}.git_upload_blob_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    upload_duration: dynamic_histogram("{}.upload_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    download_duration: dynamic_histogram("{}.download_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    download_sha256_duration: dynamic_histogram("{}.download_sha256_ms", (repo: String); 100, 0, 5000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    batch_duration: dynamic_histogram("{}.batch_ms", (repo: String); 10, 0, 500, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    response_bytes_sent: dynamic_histogram("{}.response_bytes_sent", (repo_and_method: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    // Not all requests have a valid method and repo, so calculate the top level HTTP stats first.
    STATS::requests.add_value(1);
    if status.is_success() {
        STATS::success.add_value(1);
    } else if status.is_client_error() {
        if status == StatusCode::TOO_MANY_REQUESTS {
            STATS::failure_429.add_value(1);
        } else if status == StatusCode::NOT_FOUND {
            STATS::failure_404.add_value(1);
        }

        STATS::failure_4xx.add_value(1);
    } else if status.is_server_error() {
        STATS::failure_5xx.add_value(1);
    }

    let ctx = state.try_borrow::<RequestContext>()?;
    let method = ctx.method?;
    let repo = ctx.repository.clone()?;
    let repo_and_method = format!("{}.{}", &repo, method);

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;

    callbacks.add(move |info| {
        if let Some(duration) = info.duration {
            match method {
                LfsMethod::Upload => {
                    STATS::upload_duration.add_value(duration.as_millis_unchecked() as i64, (repo,))
                }
                LfsMethod::Download => STATS::download_duration
                    .add_value(duration.as_millis_unchecked() as i64, (repo,)),
                LfsMethod::DownloadSha256 => STATS::download_sha256_duration
                    .add_value(duration.as_millis_unchecked() as i64, (repo,)),
                LfsMethod::Batch => {
                    STATS::batch_duration.add_value(duration.as_millis_unchecked() as i64, (repo,))
                }
                LfsMethod::GitBlob => STATS::git_upload_blob_duration
                    .add_value(duration.as_millis_unchecked() as i64, (repo,)),
            }
        }

        STATS::repo_requests.add_value(1, (repo_and_method.clone(),));

        if status.is_success() {
            STATS::repo_success.add_value(1, (repo_and_method.clone(),));
        } else if status.is_client_error() {
            STATS::repo_failure_4xx.add_value(1, (repo_and_method.clone(),));
        } else if status.is_server_error() {
            STATS::repo_failure_5xx.add_value(1, (repo_and_method.clone(),));
        }

        if let Some(response_bytes_sent) = info.meta.as_ref().map(|m| m.body().bytes_sent) {
            STATS::response_bytes_sent.add_value(response_bytes_sent as i64, (repo_and_method,))
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

#[async_trait::async_trait]
impl Middleware for OdsMiddleware {
    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(ctx) = state.try_borrow::<RequestContext>() {
            if ctx.should_log {
                log_stats(state, response.status());
            }
        }
    }
}
