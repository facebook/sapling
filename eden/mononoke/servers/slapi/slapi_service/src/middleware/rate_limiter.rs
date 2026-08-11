/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use backend_if::RimBackend;
use context::CoreContext;
use gotham::handler::IntoBody as _;
use gotham::helpers::http::Body;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::request_context::RequestContext;
use http::Response;
use http::StatusCode;
use http::Uri;
use maplit::hashmap;
use permission_checker::MononokeIdentitySet;
use permission_checker::TenantInfo;
use rate_limiting::Metric;
use rate_limiting::RateLimitStatus;
use tracing::debug;

use crate::utils::build_counter;
use crate::utils::counter_check_and_bump;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::RimQpsDecision;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::check_qps;
#[cfg(fbcode_build)]
use crate::utils::rim_rate_limiter::report_qps;

const EDENAPI_QPS_LIMIT: &str = "edenapi_qps";

#[cfg(fbcode_build)]
enum QpsEnforcer {
    Legacy,
    Rim,
}

#[cfg(fbcode_build)]
fn qps_enforcer() -> QpsEnforcer {
    if justknobs::eval("scm/mononoke:slapi_rim_enforce", None, None) {
        QpsEnforcer::Rim
    } else {
        QpsEnforcer::Legacy
    }
}

#[derive(Debug, Eq, PartialEq)]
enum LegacyQpsDecision {
    Allow,
    Reject(String),
}

fn too_many_requests(message: impl ToString) -> Response<Body> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .body(message.to_string().into_body())
        .expect("Couldn't build http response")
}

fn legacy_enforcement_response(decision: Option<LegacyQpsDecision>) -> Option<Response<Body>> {
    match decision {
        Some(LegacyQpsDecision::Reject(message)) => Some(too_many_requests(message)),
        Some(LegacyQpsDecision::Allow) | None => None,
    }
}

#[cfg(fbcode_build)]
fn rim_enforcement_response(decision: RimQpsDecision) -> Option<Response<Body>> {
    match decision {
        RimQpsDecision::Reject => Some(too_many_requests("RIM QPS rate limit exceeded")),
        RimQpsDecision::Allow | RimQpsDecision::FailOpen => None,
    }
}

#[cfg(fbcode_build)]
fn enforcement_response(
    enforcer: QpsEnforcer,
    rim_decision: Option<RimQpsDecision>,
    legacy_decision: Option<LegacyQpsDecision>,
) -> Option<Response<Body>> {
    match enforcer {
        QpsEnforcer::Legacy => legacy_enforcement_response(legacy_decision),
        QpsEnforcer::Rim => rim_decision.and_then(rim_enforcement_response),
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

        let rctx: RequestContext = RequestContext::borrow_from(state).clone();
        let ctx: CoreContext = rctx.ctx;

        let metadata = state.try_borrow::<MetadataState>()?.metadata();
        let tenant = metadata.tenant_info();
        let identities = metadata.identities();
        let clientinfo_atlas = metadata.clientinfo_atlas();

        #[cfg(fbcode_build)]
        let enforcer = qps_enforcer();

        #[cfg(fbcode_build)]
        return if let Some(rim_backend) = self.rim_backend {
            let (rim_decision, (), legacy_decision) = tokio::join!(
                check_qps(&ctx, &tenant, rim_backend),
                report_qps(&ctx, &tenant, rim_backend),
                check_and_report_legacy_qps(&ctx, identities, clientinfo_atlas, &tenant),
            );

            enforcement_response(enforcer, Some(rim_decision), legacy_decision)
        } else {
            let legacy_decision =
                check_and_report_legacy_qps(&ctx, identities, clientinfo_atlas, &tenant).await;
            enforcement_response(enforcer, None, legacy_decision)
        };

        #[cfg(not(fbcode_build))]
        legacy_enforcement_response(
            check_and_report_legacy_qps(&ctx, identities, clientinfo_atlas, &tenant).await,
        )
    }
}

#[cfg(all(test, fbcode_build))]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn missing_rim_backend_fails_open_when_rim_enforces() {
        let legacy_decision = Some(LegacyQpsDecision::Reject("legacy rejected".to_string()));

        assert!(enforcement_response(QpsEnforcer::Rim, None, legacy_decision).is_none());
    }
}

async fn check_and_report_legacy_qps(
    ctx: &CoreContext,
    identities: &MononokeIdentitySet,
    clientinfo_atlas: Option<bool>,
    tenant: &TenantInfo,
) -> Option<LegacyQpsDecision> {
    let rate_limiter = ctx.session().rate_limiter().or_else(|| {
        debug!("No rate_limiter info found");
        None
    })?;

    // No main id -> this request can't be attributed to a client, so it
    // isn't subject to per-client throttling.
    let Some(client_main_id) = tenant.client_id.as_deref() else {
        debug!("No main client id found");
        return None;
    };

    let limit = rate_limiter.find_rate_limit(
        Metric::EdenApiQps,
        Some(identities.clone()),
        Some(client_main_id),
        clientinfo_atlas,
    )?;

    let enforced = match limit.body.raw_config.status {
        RateLimitStatus::Disabled => return None,
        RateLimitStatus::Tracked => false,
        RateLimitStatus::Enforced => true,
        _ => panic!("Invalid limit status: {:?}", limit.body.raw_config.status),
    };

    let category = rate_limiter.category();
    let counter = build_counter(ctx, category, EDENAPI_QPS_LIMIT, client_main_id);

    Some(
        match counter_check_and_bump(
            ctx,
            counter,
            1.0,
            limit,
            enforced,
            hashmap! {
                "client_main_id" => client_main_id,
                "client_category" => tenant.category.as_str(),
            },
        )
        .await
        {
            Ok(_) => LegacyQpsDecision::Allow,
            Err(response) => LegacyQpsDecision::Reject(response.to_string()),
        },
    )
}
