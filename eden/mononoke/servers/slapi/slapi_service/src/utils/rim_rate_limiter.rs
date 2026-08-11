/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! RIM-backed rate limiting on the EdenAPI QPS path. Supports both comparison
//! against the legacy rate limiter and authoritative enforcement. RIM failures
//! always fail open.
//!
//! Configerator source of truth: `source/rim/backend_settings/mononoke_server/`.

use std::collections::HashMap;
use std::time::Duration;

use backend_if::RimBackend;
use context::CoreContext;
use permission_checker::TenantInfo;
use rim_ligen::RimThinClient;
use tokio::time::timeout;
use tracing::debug;
use tracing::warn;

const RIM_RESOURCE_QPS: &str = "qps";
const RIM_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RimQpsDecision {
    Allow,
    Reject,
    FailOpen,
}

/// Call once at server startup. Failures are logged and swallowed so RIM
/// infra outages can't prevent the server from serving traffic.
pub(crate) fn init(rim_backend: RimBackend) {
    match RimThinClient::initialize(rim_backend) {
        Ok(status) if status.success() => {
            debug!("RIM thin client initialized for backend {:?}", rim_backend);
        }
        Ok(status) => {
            warn!(
                "RIM initialize returned non-success: code={:?} msg={}",
                status.code(),
                status.message(),
            );
        }
        Err(e) => {
            warn!("RIM initialize failed for backend {:?}: {}", rim_backend, e);
        }
    }
}

pub(crate) async fn check_qps(
    ctx: &CoreContext,
    tenant: &TenantInfo,
    rim_backend: RimBackend,
) -> RimQpsDecision {
    let Some(tenancy_path) = tenant.tenancy_path() else {
        return RimQpsDecision::FailOpen;
    };
    let requirements = HashMap::from([(RIM_RESOURCE_QPS.to_string(), 1.0)]);

    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add("rim_tenancy_path", tenant.to_string());
        scuba.log_with_msg(tag, detail);
    };

    let result = match timeout(
        RIM_ACQUIRE_TIMEOUT,
        RimThinClient::acquire(rim_backend, tenancy_path, requirements),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            log("RIM QPS check error", e.to_string());
            return RimQpsDecision::FailOpen;
        }
        Err(_) => {
            log(
                "RIM QPS check timeout",
                format!("timeout after {RIM_ACQUIRE_TIMEOUT:?}"),
            );
            return RimQpsDecision::FailOpen;
        }
    };

    if result.rejected() {
        log(
            "RIM rejected QPS request",
            format!("code={:?}", result.code()),
        );
        RimQpsDecision::Reject
    } else if result.failed() {
        log("RIM QPS check failed", format!("code={:?}", result.code()));
        RimQpsDecision::FailOpen
    } else {
        RimQpsDecision::Allow
    }
}

pub(crate) async fn report_qps(ctx: &CoreContext, tenant: &TenantInfo, rim_backend: RimBackend) {
    let Some(tenancy_path) = tenant.tenancy_path() else {
        return;
    };
    let usage = HashMap::from([(RIM_RESOURCE_QPS.to_string(), 1.0)]);

    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add("rim_tenancy_path", tenant.to_string());
        scuba.log_with_msg(tag, detail);
    };

    match timeout(
        RIM_ACQUIRE_TIMEOUT,
        RimThinClient::report(rim_backend, tenancy_path, usage),
    )
    .await
    {
        Ok(Ok(result)) if !result.success() => {
            log("RIM report non-success", format!("{:?}", result.status()));
        }
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            log("RIM report error", e.to_string());
        }
        Err(_) => {
            log(
                "RIM report timeout",
                format!("timeout after {RIM_ACQUIRE_TIMEOUT:?}"),
            );
        }
    }
}
