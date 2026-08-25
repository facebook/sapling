/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use anyhow::anyhow;
#[cfg(fbcode_build)]
use backend_if::RimBackend;
use context::CoreContext;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
#[cfg(fbcode_build)]
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::response::build_error_response_in_place;
use http::Response;
use http::StatusCode;
use http::Uri;
#[cfg(fbcode_build)]
use permission_checker::TenantInfo;

use crate::handlers::JsonErrorFormatter;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimQpsDecision;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::check_qps;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::report_qps;

#[cfg(fbcode_build)]
const RIM_ENFORCE_JK: &str = "scm/mononoke:slapi_rim_enforce";

#[cfg(fbcode_build)]
const RIM_REJECTION_MESSAGE: &str = "RIM QPS rate limit exceeded";

#[cfg(fbcode_build)]
fn rim_rejection_response(state: &mut State) -> Response<Body> {
    error_response(state, HttpError::e429(anyhow!(RIM_REJECTION_MESSAGE)))
}

fn error_response(state: &mut State, error: HttpError) -> Response<Body> {
    match build_error_response_in_place(error, state, &JsonErrorFormatter) {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(?error, "Failed to build error response");
            let mut response = Response::new(Body::default());
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            response
        }
    }
}

#[cfg(fbcode_build)]
async fn apply_rim_decision(
    state: &mut State,
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
            Some(rim_rejection_response(state))
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

fn load_shedding_response(state: &mut State, ctx: &CoreContext) -> Option<Response<Body>> {
    let mut scuba = ctx.scuba().clone();
    ctx.session()
        .check_load_shed(&mut scuba)
        .err()
        .map(|error| error_response(state, error.into()))
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

        let rctx: RequestContext = RequestContext::borrow_from(state).clone();
        let ctx: CoreContext = rctx.ctx;

        if let Some(response) = load_shedding_response(state, &ctx) {
            return Some(response);
        }

        #[cfg(fbcode_build)]
        if let Some(rim_backend) = self.rim_backend {
            let tenant = state
                .try_borrow::<MetadataState>()
                .map(|metadata| metadata.metadata().tenant_info());
            if let Some(tenant) = tenant {
                let rim_decision = check_qps(&ctx, &tenant, rim_backend).await;

                if let Some(response) =
                    apply_rim_decision(state, &ctx, &tenant, rim_backend, rim_decision).await
                {
                    return Some(response);
                }
            }
        }

        None
    }
}
