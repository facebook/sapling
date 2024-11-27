/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_requests::RequestId;
use context::CoreContext;
use slog::info;

use crate::worker::AsyncMethodRequestWorker;

impl AsyncMethodRequestWorker {
    pub(crate) fn prepare_ctx(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        target: &str,
    ) -> CoreContext {
        let ctx = ctx.with_mutated_scuba(|mut scuba| {
            // Legacy column logging the token as an integer
            scuba.add("request_id", req_id.0.0);
            // New name to match the mononoke_scs_server table
            scuba.add("token", format!("{}", req_id.0.0));
            scuba.add("request_type", req_id.1.0.clone());
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
