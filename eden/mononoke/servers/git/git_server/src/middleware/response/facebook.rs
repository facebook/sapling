// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use std::sync::LazyLock;

use MononokeGitRequest_ods3::Instrument_MononokeGitRequest;
use MononokeGitRequest_ods3_types::GitRequestOutcome;
use MononokeGitRequest_ods3_types::MononokeGitRequest;
use gotham_ext::middleware::post_request::PostResponseInfo;
use http::StatusCode;

static GIT_REQUEST_INSTRUMENT: LazyLock<Instrument_MononokeGitRequest> =
    LazyLock::new(Instrument_MononokeGitRequest::new);

pub fn log_ods3(
    info: &PostResponseInfo,
    status: &StatusCode,
    method: String,
    method_variants: String,
    repo: String,
    request_load: Option<f64>,
) {
    let outcome = if status.is_success() {
        Some(GitRequestOutcome::Success)
    } else if status.is_client_error() {
        Some(GitRequestOutcome::Failure_4xx)
    } else if status.is_server_error() {
        Some(GitRequestOutcome::Failure_5xx)
    } else {
        None
    };

    GIT_REQUEST_INSTRUMENT.observe(MononokeGitRequest {
        git_method: Some(method),
        git_method_variant: Some(method_variants),
        repo: Some(repo),
        outcome,
        request_load,
        requests: Some(1.0),
        response_bytes_sent: info.meta.as_ref().map(|m| m.body().bytes_sent as f64),
        duration_ms: info.duration.map(|d| d.as_millis() as f64),
        ..Default::default()
    });
}
