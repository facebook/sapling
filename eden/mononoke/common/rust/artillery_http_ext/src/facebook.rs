/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use tracer::ManagedTraceScopeGuard;
use tracer::Tracer;
use tracer::deserialize_artillery_propagated_context;

const ARTILLERY_POLICY: &str = "mononoke_scm";

pub struct ArtilleryTraceGuard(
    #[expect(
        dead_code,
        reason = "RAII guard — held for its Drop impl to keep the Artillery trace active for the request lifetime"
    )]
    ManagedTraceScopeGuard,
);

/// Continue an inbound Artillery trace from the `propagated-artillery-trace-ids`
/// HTTP header, if present. Returned guard MUST be held for the duration of the request.
pub fn continue_trace_from_context(
    fb: FacebookInit,
    context_bytes: &[u8],
) -> Option<ArtilleryTraceGuard> {
    let ctx = deserialize_artillery_propagated_context(context_bytes)?;
    Tracer::new(fb, ARTILLERY_POLICY)
        .continue_managed_trace(&ctx)
        .ok()
        .map(ArtilleryTraceGuard)
}

pub fn current_trace_id() -> Option<String> {
    let trace_id = service_tracing::get_trace_id();
    if trace_id.is_empty() {
        None
    } else {
        Some(trace_id)
    }
}
