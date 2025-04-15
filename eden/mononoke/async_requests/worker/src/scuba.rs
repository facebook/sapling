/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use async_requests::RequestId;
use async_requests::types::AsynchronousRequestResult;
use async_requests_types_thrift::AsynchronousRequestResult as ThriftAsynchronousRequestResult;
use context::CoreContext;
use futures_stats::FutureStats;
use slog::info;
use source_control::AsyncRequestError;
use stats::define_stats;
use stats::prelude::*;

use crate::worker::AsyncMethodRequestWorker;

define_stats! {
    prefix = "async_requests.worker.process";

    process_complete_failed: timeseries("complete.failed"; Count),
    process_retriable_error: timeseries("retriable.error"; Count),
    process_succeeded: timeseries("succeeded"; Count),
    process_error: timeseries("error"; Count),
}

impl AsyncMethodRequestWorker {
    pub(crate) fn prepare_ctx(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        target: &str,
    ) -> CoreContext {
        let ctx = ctx.with_mutated_scuba(|mut scuba| {
            // Legacy columns
            scuba.add("request_id", req_id.0.0);
            scuba.add("request_type", req_id.1.0.clone());

            // New column names to match the mononoke_scs_server table
            scuba.add("token", format!("{}", req_id.0.0));
            scuba.add("method", req_id.1.0.clone());
            scuba
        });

        info!(
            ctx.logger(),
            "[{}] new request:  id: {}, type: {}, {}", &req_id.0, &req_id.0, &req_id.1, target,
        );

        ctx
    }
}

pub(crate) fn log_start(ctx: &CoreContext) {
    let mut scuba = ctx.scuba().clone();
    scuba.log_with_msg("Request start", None);
}

/// Log the result of a request: either a success or a final error. Retriable errors (i.e. where the worker
/// internally failed and will retry) are logged separately.
pub(crate) fn log_result(
    ctx: CoreContext,
    stats: &FutureStats,
    result: &AsynchronousRequestResult,
    complete_result: &Result<bool>,
) {
    let mut scuba = ctx.scuba().clone();

    let (status, error, succeeded, complete_failed, method_error) = match result.thrift() {
        ThriftAsynchronousRequestResult::error(error) => match error {
            AsyncRequestError::request_error(error) => {
                ("REQUEST_ERROR", Some(format!("{:?}", error)), 0, 0, 1)
            }
            AsyncRequestError::internal_error(error) => {
                ("INTERNAL_ERROR", Some(format!("{:?}", error)), 0, 0, 1)
            }
            AsyncRequestError::UnknownField(error) => (
                "UNKNOWN_ERROR",
                Some(format!("unknown error: {:?}", error)),
                0,
                0,
                1,
            ),
        },
        _ => match complete_result {
            Ok(_complete) => ("SUCCESS", None, 1, 0, 0),
            Err(err) => ("SAVE_ERROR", Some(err.to_string()), 0, 1, 0),
        },
    };

    STATS::process_succeeded.add_value(succeeded);
    STATS::process_complete_failed.add_value(complete_failed);
    STATS::process_error.add_value(method_error);

    scuba.add_future_stats(stats);
    scuba.add("status", status);

    if let Some(error) = error {
        scuba.unsampled();
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg("Request complete", None);
}

/// Log a retriable error, i.e. one that failed because of internal worker issues and will be retried.
pub(crate) fn log_retriable_error(ctx: CoreContext, stats: &FutureStats, error: Error) {
    let mut scuba = ctx.scuba().clone();

    STATS::process_retriable_error.add_value(1);

    scuba.add_future_stats(stats);
    scuba.add("status", "RETRIABLE_ERROR");
    scuba.unsampled();
    scuba.add("error", format!("{:?}", error));

    scuba.log_with_msg("Request complete", None);
}
