/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use std::collections::HashMap;

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

use crate::handlers::JsonErrorFormatter;
use crate::middleware::ods::record_processed_work;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimDecision;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimRateLimiter;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::STORAGE_READS_RESOURCE;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::check_rate_limit;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::record_usage;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::report_request;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::request_attribution;

#[cfg(fbcode_build)]
const RIM_ENFORCE_JK: &str = "scm/mononoke:slapi_rim_enforce";

#[cfg(fbcode_build)]
const RIM_REJECTION_MESSAGE: &str = "RIM rate limit exceeded";

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
    tenancy_path: &[String],
    rim_backend: RimBackend,
    requirements: &HashMap<String, f64>,
    selection_key: Option<&str>,
) -> Option<Response<Body>> {
    let decision =
        check_rate_limit(ctx, tenancy_path, rim_backend, requirements, selection_key).await;
    let rejected = decision == RimDecision::Reject && justknobs::eval(RIM_ENFORCE_JK, None, None);
    if decision != RimDecision::FailOpen {
        report_request(ctx, tenancy_path, rim_backend, requirements, rejected);
    }
    match decision {
        RimDecision::Allow => None,
        RimDecision::Reject if rejected => Some(rim_rejection_response(state)),
        RimDecision::Reject => {
            let mut scuba = ctx.scuba().clone();
            scuba.add("rim_tenancy_path", tenancy_path.join("/"));
            scuba.add("rim_backend", rim_backend.0);
            scuba.log_with_msg(
                "RIM would have rejected request but enforcement is disabled",
                format!("JustKnob {RIM_ENFORCE_JK} is disabled"),
            );
            None
        }
        RimDecision::FailOpen => None,
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
    rim: Option<RimRateLimiter>,
}
impl ThrottleMiddleware {
    pub(crate) fn new(#[cfg(fbcode_build)] rim: Option<RimRateLimiter>) -> Self {
        Self {
            #[cfg(fbcode_build)]
            rim,
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
        if let Some(rim) = &self.rim
            && let Some(metadata) = state.try_borrow::<MetadataState>()
        {
            let config = rim.config.get();
            let (path, selection_key) = request_attribution(metadata.metadata(), &config);
            // With demand_equals_usage=false, acquire selects resources; completion reports usage.
            let requirements = [
                (rim.cpu_resource.clone(), 0.0),
                (STORAGE_READS_RESOURCE.to_owned(), 0.0),
            ]
            .into();
            if let Some(response) = apply_rim_decision(
                state,
                &ctx,
                &path,
                rim.backend,
                &requirements,
                selection_key.as_deref(),
            )
            .await
            {
                return Some(response);
            }
            record_usage(
                state,
                ctx.clone(),
                rim.backend,
                path,
                rim.cpu_resource.clone(),
            );
        }

        record_processed_work(state, ctx);
        None
    }
}
