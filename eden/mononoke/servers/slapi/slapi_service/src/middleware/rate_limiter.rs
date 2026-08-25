/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use backend_if::RimBackend;
#[cfg(fbcode_build)]
use context::CoreContext;
#[cfg(fbcode_build)]
use gotham::handler::IntoBody as _;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
#[cfg(fbcode_build)]
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
#[cfg(fbcode_build)]
use gotham_ext::middleware::request_context::RequestContext;
use http::Response;
#[cfg(fbcode_build)]
use http::StatusCode;
use http::Uri;
#[cfg(fbcode_build)]
use permission_checker::TenantInfo;

#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimQpsDecision;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::check_qps;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::report_qps;

#[cfg(fbcode_build)]
const RIM_ENFORCE_JK: &str = "scm/mononoke:slapi_rim_enforce";

#[cfg(fbcode_build)]
fn too_many_requests(message: impl ToString) -> Response<Body> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .body(message.to_string().into_body())
        .expect("Couldn't build http response")
}

#[cfg(fbcode_build)]
async fn apply_rim_decision(
    ctx: &CoreContext,
    tenant: &TenantInfo,
    rim_backend: RimBackend,
    decision: RimQpsDecision,
) -> Option<Response<Body>> {
    match decision {
        RimQpsDecision::Allow => {
            report_qps(ctx, tenant, rim_backend).await;
            None
        }
        RimQpsDecision::Reject if justknobs::eval(RIM_ENFORCE_JK, None, None) => {
            Some(too_many_requests("RIM QPS rate limit exceeded"))
        }
        RimQpsDecision::Reject => {
            let mut scuba = ctx.scuba().clone();
            scuba.add("rim_tenancy_path", tenant.to_string());
            scuba.log_with_msg(
                "RIM would have rejected QPS request but enforcement is disabled",
                format!("JustKnob {RIM_ENFORCE_JK} is disabled"),
            );
            report_qps(ctx, tenant, rim_backend).await;
            None
        }
        RimQpsDecision::FailOpen => None,
    }
}

// NOTE: Our Throttling middleware is implemented as Gotham middleware for 3 reasons:
// - It needs to replace responses.
// - It needs to do asynchronously.
// - It only needs to run if we're going to serve a request.

#[derive(Clone)]
pub struct ThrottleMiddleware {
    #[cfg(fbcode_build)]
    rim_backend: Option<RimBackend>,
}
impl ThrottleMiddleware {
    pub fn new(#[cfg(fbcode_build)] rim_backend: Option<RimBackend>) -> Self {
        Self {
            #[cfg(fbcode_build)]
            rim_backend,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for ThrottleMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        if let Some(uri) = Uri::try_borrow_from(state) {
            if uri.path() == "/health_check" || uri.path() == "/proxygen/health_check" {
                return None;
            }
        }

        #[cfg(fbcode_build)]
        {
            let rim_backend = self.rim_backend?;
            let rctx: RequestContext = RequestContext::borrow_from(state).clone();
            let ctx: CoreContext = rctx.ctx;
            let metadata = state.try_borrow::<MetadataState>()?.metadata();
            let tenant = metadata.tenant_info();
            let rim_decision = check_qps(&ctx, &tenant, rim_backend).await;

            apply_rim_decision(&ctx, &tenant, rim_backend, rim_decision).await
        }

        #[cfg(not(fbcode_build))]
        {
            None
        }
    }
}
