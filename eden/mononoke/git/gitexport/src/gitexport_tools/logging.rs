/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::Future;
use futures_stats::TimedFutureExt;
use mononoke_api::CoreContext;

pub async fn run_and_log_stats_to_scuba<R>(
    ctx: &CoreContext,
    log_tag: &str,
    fut: impl Future<Output = R>,
) -> R {
    let (stats, result) = fut.timed().await;
    let mut scuba = ctx.scuba().clone();
    scuba.add_future_stats(&stats);
    scuba.log_with_msg(log_tag, None);
    result
}
