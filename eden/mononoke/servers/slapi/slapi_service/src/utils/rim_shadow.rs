/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Shadow-mode RIM probe on the EdenAPI QPS path — runs alongside the
//! legacy ratelim decision and never affects the response. Exists so RIM
//! vs ratelim decisions can be compared in scuba before RIM becomes
//! authoritative.
//!
//! Configerator source of truth: `source/rim/backend_settings/mononoke_server/`.

use std::collections::HashMap;
use std::time::Duration;

use backend_if::RimBackend;
use context::CoreContext;
use rim_ligen::RimThinClient;
use tokio::time::timeout;
use tracing::debug;
use tracing::warn;

const MONONOKE_SERVER: RimBackend = RimBackend(38000001);
const RIM_RESOURCE_QPS: &str = "qps";
const RIM_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);

/// Call once at server startup. Failures are logged and swallowed so RIM
/// infra outages can't prevent the server from serving traffic — the
/// shadow probe just becomes a no-op.
pub fn init() {
    match RimThinClient::initialize(MONONOKE_SERVER) {
        Ok(status) if status.success() => {
            debug!("RIM thin client initialized for MONONOKE_SERVER");
        }
        Ok(status) => {
            warn!(
                "RIM initialize returned non-success: code={:?} msg={}",
                status.code(),
                status.message(),
            );
        }
        Err(e) => {
            warn!("RIM initialize failed for MONONOKE_SERVER: {}", e);
        }
    }
}

/// Never returns a decision — the caller must not gate the request on
/// this. Emits a scuba sample only on reject / error / timeout to keep
/// scuba quota bounded; the common allow path is silent. The "ratelim
/// reject, RIM allow" comparison is covered by ratelim's own existing
/// rejection log.
pub async fn shadow_check(ctx: &CoreContext, client_category: &str, client_main_id: &str) {
    let tenancy_path = vec![
        "root".to_string(),
        client_category.to_string(),
        client_main_id.to_string(),
    ];
    let requirements = HashMap::from([(RIM_RESOURCE_QPS.to_string(), 1.0)]);

    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add(
            "rim_tenancy_path",
            format!("root/{client_category}/{client_main_id}"),
        );
        scuba.log_with_msg(tag, detail);
    };

    match timeout(
        RIM_ACQUIRE_TIMEOUT,
        RimThinClient::acquire(MONONOKE_SERVER, tenancy_path, requirements),
    )
    .await
    {
        Ok(Ok(result)) if result.rejected() => {
            log("RIM would reject", format!("code={:?}", result.code()));
        }
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            log("RIM shadow probe error", e.to_string());
        }
        Err(_) => {
            log(
                "RIM shadow probe timeout",
                format!("timeout after {RIM_ACQUIRE_TIMEOUT:?}"),
            );
        }
    }
}
