/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
use rate_limiting::Metric;
use rate_limiting::RateLimitStatus;
use tracing::debug;

use crate::utils::build_counter;
use crate::utils::counter_check_and_bump;

const EDENAPI_QPS_LIMIT: &str = "edenapi_qps";

// NOTE: Our Throttling middleware is implemented as Gotham middleware for 3 reasons:
// - It needs to replace responses.
// - It needs to do asynchronously.
// - It only needs to run if we're going to serve a request.

#[derive(Clone)]
pub struct ThrottleMiddleware;
impl ThrottleMiddleware {
    pub fn new() -> Self {
        Self {}
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

        // Retrieve rate limiter
        let rate_limiter = ctx.session().rate_limiter().or_else(|| {
            debug!("No rate_limiter info found");
            None
        })?;

        let metadata = state.try_borrow::<MetadataState>()?.metadata();
        let tenant = metadata.tenant_info();
        // No main id -> this request can't be attributed to a client, so it
        // isn't subject to per-client throttling.
        let Some(client_main_id) = tenant.client_id.as_deref() else {
            debug!("No main client id found");
            return None;
        };
        let identities = metadata.identities();
        let atlas = metadata.clientinfo_atlas();

        #[cfg(fbcode_build)]
        if justknobs::eval("scm/mononoke:edenapi_qps_rim_shadow", None, None) {
            crate::utils::rim_shadow::shadow_check(&ctx, &tenant).await;
        }

        let limit = rate_limiter.find_rate_limit(
            Metric::EdenApiQps,
            Some(identities.clone()),
            Some(client_main_id),
            atlas,
        )?;

        let enforced = match limit.body.raw_config.status {
            RateLimitStatus::Disabled => return None,
            RateLimitStatus::Tracked => false,
            RateLimitStatus::Enforced => true,
            _ => panic!("Invalid limit status: {:?}", limit.body.raw_config.status),
        };

        let category = rate_limiter.category();
        let counter = build_counter(&ctx, category, EDENAPI_QPS_LIMIT, client_main_id);

        match counter_check_and_bump(
            &ctx,
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
            Ok(_) => {
                #[cfg(fbcode_build)]
                if justknobs::eval("scm/mononoke:edenapi_qps_rim_shadow", None, None) {
                    crate::utils::rim_shadow::report_qps(&ctx, &tenant).await;
                }
                None
            }
            Err(response) => {
                // Per-user rate limiting (counter keyed by client_main_id):
                // always 429, this client specifically is the offender.
                let res = Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(response.to_string().into_body())
                    .expect("Couldn't build http response");
                Some(res)
            }
        }
    }
}
