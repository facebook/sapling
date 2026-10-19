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
#[cfg(fbcode_build)]
use edenapi_types::file::FILE_COUNT_HEADER;
#[cfg(fbcode_build)]
use edenapi_types::tree::TREE_COUNT_HEADER;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
#[cfg(fbcode_build)]
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::response::build_error_response_in_place;
#[cfg(fbcode_build)]
use http::HeaderMap;
#[cfg(fbcode_build)]
use http::Method;
use http::Response;
use http::StatusCode;
use http::Uri;

use crate::handlers::JsonErrorFormatter;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimDecision;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::WorkloadConfig;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::check_rate_limit;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::legacy_tenancy_path;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::report_load;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::workload_tenancy_path;

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
) -> Option<Response<Body>> {
    let decision = check_rate_limit(ctx, tenancy_path, rim_backend, requirements).await;
    // The deployed backends use demand_equals_usage: report all work offered
    // to this stage, including requests this stage rejects.
    if decision != RimDecision::FailOpen {
        report_load(ctx, tenancy_path, rim_backend, requirements).await;
    }
    match decision {
        RimDecision::Allow => None,
        RimDecision::Reject if justknobs::eval(RIM_ENFORCE_JK, None, None) => {
            Some(rim_rejection_response(state))
        }
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
    rim_backend: Option<RimBackend>,
    #[cfg(fbcode_build)]
    rim_regional_backend: Option<(RimBackend, String)>,
    #[cfg(fbcode_build)]
    rim_workload_config: Option<WorkloadConfig>,
}
impl ThrottleMiddleware {
    pub fn new(
        #[cfg(fbcode_build)] rim_backend: Option<RimBackend>,
        #[cfg(fbcode_build)] rim_regional_backend: Option<(RimBackend, String)>,
        #[cfg(fbcode_build)] rim_workload_config: Option<WorkloadConfig>,
    ) -> Self {
        Self {
            #[cfg(fbcode_build)]
            rim_backend,
            #[cfg(fbcode_build)]
            rim_regional_backend,
            #[cfg(fbcode_build)]
            rim_workload_config,
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
        if self.rim_backend.is_some() || self.rim_regional_backend.is_some() {
            let scopes = [
                self.rim_regional_backend
                    .as_ref()
                    .map(|(backend, region)| (*backend, Some(region.as_str()))),
                self.rim_backend.map(|backend| (backend, None)),
            ];
            let paths = state.try_borrow::<MetadataState>().map(|metadata| {
                scopes.map(|scope| {
                    let (backend, region) = scope?;
                    let path = if let Some(config) = &self.rim_workload_config {
                        let path = metadata.metadata().rim_tenancy_path(region);
                        workload_tenancy_path(path, &config.get(), backend)
                    } else {
                        legacy_tenancy_path(&metadata.metadata().tenant_info(), region)?
                    };
                    Some((backend, path))
                })
            });
            if let Some(paths) = paths {
                // Older clients keep QPS-only accounting. Fetch counts are advisory.
                let fetches = (Method::borrow_from(state) == Method::POST)
                    .then(|| {
                        // The API prefix is stripped; the encoded repo is one path segment.
                        let (_, endpoint) = Uri::borrow_from(state)
                            .path()
                            .strip_prefix('/')?
                            .split_once('/')?;
                        let (header, resource) = match endpoint {
                            "files2" => (FILE_COUNT_HEADER, "file_fetches"),
                            "trees" => (TREE_COUNT_HEADER, "tree_fetches"),
                            _ => return None,
                        };
                        let count = HeaderMap::borrow_from(state)
                            .get(header)?
                            .to_str()
                            .ok()?
                            .parse::<u32>()
                            .ok()?;
                        Some((resource, f64::from(count)))
                    })
                    .flatten();
                let requirements = [Some(("qps", 1.0)), fetches]
                    .into_iter()
                    .flatten()
                    .map(|(resource, units)| (resource.to_owned(), units))
                    .collect();
                // Global accounting sees only traffic that proceeds past the regional stage.
                for (backend, path) in paths.into_iter().flatten() {
                    if let Some(response) =
                        apply_rim_decision(state, &ctx, &path, backend, &requirements).await
                    {
                        return Some(response);
                    }
                }
            }
        }

        None
    }
}
