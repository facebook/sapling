/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use async_requests::types::RequestStatus;
use async_requests::AsyncMethodRequestQueue;
use context::CoreContext;
use mononoke_types::Timestamp;
use slog::warn;
use stats::define_stats;
use stats::prelude::*;

const STATS_LOOP_INTERNAL: Duration = Duration::from_secs(5 * 60);

const STATUSES: [RequestStatus; 4] = [
    RequestStatus::New,
    RequestStatus::InProgress,
    RequestStatus::Ready,
    RequestStatus::Polled,
];

define_stats! {
    prefix = "async_requests.worker.stats";

    stats_error: timeseries("error"; Count),
    queue_length_by_status: dynamic_singleton_counter("queue.length.{}", (status: String)),
    queue_age_by_status: dynamic_singleton_counter("queue.age_s.{}", (status: String)),
}

pub(crate) async fn stats_loop(ctx: &CoreContext, queue: &AsyncMethodRequestQueue) {
    loop {
        let now = Timestamp::now();
        let res = queue.get_queue_stats(ctx).await;
        match res {
            Ok(res) => {
                for status in STATUSES {
                    let count = res.queue_length_by_status.get(&status).unwrap_or(&0);
                    STATS::queue_length_by_status.set_value(
                        ctx.fb,
                        *count as i64,
                        (status.to_string(),),
                    );

                    let ts = res.queue_age_by_status.get(&status).unwrap_or(&now);
                    let diff = now.timestamp_seconds() - ts.timestamp_seconds();
                    STATS::queue_age_by_status.set_value(ctx.fb, diff, (status.to_string(),));
                }
            }
            Err(err) => {
                STATS::stats_error.add_value(1);
                warn!(
                    ctx.logger(),
                    "error while getting queue stats, skipping: {}", err
                );
            }
        }

        tokio::time::sleep(STATS_LOOP_INTERNAL).await;
    }
}
