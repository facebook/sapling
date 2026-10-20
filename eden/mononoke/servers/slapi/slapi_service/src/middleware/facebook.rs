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
    batch_size: Option<f64>,
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
    let duration_ms = info.duration.map(|d| d.as_millis() as f64);
    SAPLING_API_INSTRUMENT.observe(MononokeSaplingApiRequest {
        sapling_api_method: Some(method),
        duration_ms,
        outcome,
        response_bytes_sent: info.meta.as_ref().map(|m| m.body().bytes_sent as f64),
        requests: Some(1.0),
        request_load,
        batch_size,
        duration_ms_per_item: per_item_duration_ms(duration_ms, batch_size),
        ..Default::default()
    });
}

pub(crate) fn per_item_duration_ms(
    duration_ms: Option<f64>,
    batch_size: Option<f64>,
) -> Option<f64> {
    match (duration_ms, batch_size) {
        (Some(duration_ms), Some(batch_size)) if batch_size >= 1.0 => {
            Some(duration_ms / batch_size)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn per_item_divides_duration_by_batch() {
        assert_eq!(
            per_item_duration_ms(Some(100.0), Some(4.0)),
            Some(25.0),
            "100ms over 4 files is 25ms per file"
        );
    }

    #[mononoke::test]
    fn per_item_absent_without_duration() {
        assert_eq!(per_item_duration_ms(None, Some(4.0)), None);
    }

    #[mononoke::test]
    fn per_item_absent_without_batch() {
        assert_eq!(per_item_duration_ms(Some(100.0), None), None);
    }

    #[mononoke::test]
    fn per_item_absent_for_zero_batch() {
        assert_eq!(
            per_item_duration_ms(Some(100.0), Some(0.0)),
            None,
            "zero batch must not divide"
        );
    }
}
