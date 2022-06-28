/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::StreamExt;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use slog::warn;
use std::sync::Arc;
use std::time::Duration;

const SUBMIT_STATS_ONCE_PER_SECS: u64 = 10;

pub async fn monitoring_stats_submitter(ctx: CoreContext, mononoke: Arc<Mononoke>) {
    tokio_shim::time::interval_stream(Duration::from_secs(SUBMIT_STATS_ONCE_PER_SECS))
        .for_each(|_| async {
            if let Err(e) = mononoke.report_monitoring_stats(&ctx).await {
                warn!(ctx.logger(), "Failed to report monitoring stats: {:#?}", e);
            }
        })
        .await;
}
