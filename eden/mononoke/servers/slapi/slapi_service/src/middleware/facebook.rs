// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use std::sync::LazyLock;

use MononokeSaplingApiRequest_ods3::Instrument_MononokeSaplingApiRequest;
use MononokeSaplingApiRequest_ods3_types::MononokeSaplingApiRequest;
use MononokeSaplingApiRequest_ods3_types::SaplingApiRequestOutcome;
use gotham_ext::middleware::post_request::PostResponseInfo;
use http::StatusCode;

static SAPLING_API_INSTRUMENT: LazyLock<Instrument_MononokeSaplingApiRequest> =
    LazyLock::new(Instrument_MononokeSaplingApiRequest::new);

pub fn log_ods3(
    info: &PostResponseInfo,
    status: &StatusCode,
    method: String,
    request_load: Option<f64>,
) {
    let outcome = if status.is_success() {
        Some(SaplingApiRequestOutcome::Success)
    } else if status.is_client_error() {
        Some(SaplingApiRequestOutcome::Failure_4xx)
    } else if status.is_server_error() {
        Some(SaplingApiRequestOutcome::Failure_5xx)
    } else {
        None
    };
    SAPLING_API_INSTRUMENT.observe(MononokeSaplingApiRequest {
        sapling_api_method: Some(method),
        duration_ms: info.duration.map(|d| d.as_millis() as f64),
        outcome,
        response_bytes_sent: info.meta.as_ref().map(|m| m.body().bytes_sent as f64),
        requests: Some(1.0),
        request_load,
        ..Default::default()
    });
}
