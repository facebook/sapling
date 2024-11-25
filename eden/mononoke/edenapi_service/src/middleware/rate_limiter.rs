/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use hyper::Uri;
use maplit::hashmap;
use rate_limiting::Metric;
use rate_limiting::RateLimitStatus;
use sha2::Digest;
use sha2::Sha256;
use slog::debug;
use time_window_counter::BoxGlobalTimeWindowCounter;
use time_window_counter::GlobalTimeWindowCounterBuilder;
use tokio::time::timeout;

const TIME_WINDOW_MIN: u32 = 10;
const TIME_WINDOW_MAX: u32 = 3600;
const RATELIM_FETCH_TIMEOUT: Duration = Duration::from_secs(1);
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

        let client_request_info = state
            .try_borrow::<MetadataState>()?
            .metadata()
            .client_request_info()
            .or_else(|| {
                debug!(ctx.logger(), "No client request info found");
                None
            })?;
        // Retrieve main client ID
        let main_client_id = client_request_info.main_id.clone().or_else(|| {
            debug!(ctx.logger(), "No main client id found");
            None
        })?;
        // Retrieve rate limiter
        let rate_limiter = ctx.session().rate_limiter().or_else(|| {
            debug!(ctx.logger(), "No rate_limiter info found");
            None
        })?;

        let category = rate_limiter.category();
        let limit = rate_limiter.find_rate_limit(Metric::EdenApiQps)?;

        let enforced = match limit.body.raw_config.status {
            RateLimitStatus::Disabled => return None,
            RateLimitStatus::Tracked => false,
            RateLimitStatus::Enforced => true,
            _ => panic!("Invalid limit status: {:?}", limit.body.raw_config.status),
        };
        let counter = build_counter(&ctx, category, &main_client_id);
        let max_value = limit.body.raw_config.limit;

        match counter_check_and_bump(
            &ctx,
            counter,
            max_value,
            enforced,
            hashmap! {"main_client_id" => main_client_id.as_str() },
        )
        .await
        {
            Ok(_) => None,
            Err(response) => {
                let res = Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(response.to_string().into())
                    .expect("Couldn't build http response");
                Some(res)
            }
        }
    }
}
fn build_counter(ctx: &CoreContext, category: &str, main_id: &str) -> BoxGlobalTimeWindowCounter {
    let key = make_key(EDENAPI_QPS_LIMIT, main_id);
    debug!(
        ctx.logger(),
        "Associating key {:?} with author {:?}", key, main_id
    );
    GlobalTimeWindowCounterBuilder::build(ctx.fb, category, key, TIME_WINDOW_MIN, TIME_WINDOW_MAX)
}
fn make_key(prefix: &str, client_main_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(client_main_id);
    format!("{}.{}", prefix, hex::encode(hasher.finalize()))
}

/// Check if a counter would exceed maximum value if bumped
/// and bump it if it would not. If getting the counter
/// value times out, just act as if rate-limit check passes.
async fn counter_check_and_bump<'a>(
    ctx: &'a CoreContext,
    counter: BoxGlobalTimeWindowCounter,
    max_value: f64,
    enforced: bool,
    scuba_extras: HashMap<&'a str, &'a str>,
) -> Result<(), Error> {
    let mut scuba = ctx.scuba().clone();
    for (key, val) in scuba_extras {
        scuba.add(key, val);
    }

    match timeout(RATELIM_FETCH_TIMEOUT, counter.get(1)).await {
        Ok(Ok(count)) => {
            let new_value = count + 1.0;
            if new_value <= max_value {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) does not exceed threshold {} if bumped",
                    EDENAPI_QPS_LIMIT,
                    count,
                    max_value,
                );
                let msg = format!("{}: Passed", EDENAPI_QPS_LIMIT);
                scuba.log_with_msg(&msg, None);
                counter.bump(1.0);
                Ok(())
            } else if !enforced {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped, but enforcement is disabled",
                    EDENAPI_QPS_LIMIT,
                    count,
                    max_value,
                );
                let msg = format!("{}: Exceeded (log only)", EDENAPI_QPS_LIMIT);
                scuba.log_with_msg(&msg, None);
                Ok(())
            } else {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped. Blocking request",
                    EDENAPI_QPS_LIMIT,
                    count,
                    max_value,
                );
                let msg = format!("{}: Blocked", EDENAPI_QPS_LIMIT);
                scuba.log_with_msg(&msg, None);
                Err(anyhow!(msg))
            }
        }
        Ok(Err(e)) => {
            debug!(
                ctx.logger(),
                "Failed getting rate limiting counter {}: {:?}", EDENAPI_QPS_LIMIT, e
            );
            let msg = format!("{}: Failed", EDENAPI_QPS_LIMIT);
            scuba.log_with_msg(&msg, None);
            Ok(())
        }
        Err(_) => {
            let msg = format!("{}: Timed out", EDENAPI_QPS_LIMIT);
            scuba.log_with_msg(&msg, None);
            // Fail open to prevent DoS as we can't check the rate limit

            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use time_window_counter::GlobalTimeWindowCounter;

    use super::*;

    struct MockBoxGlobalTimeWindowCounter {
        count: Arc<Mutex<f64>>,
    }

    #[async_trait]
    impl GlobalTimeWindowCounter for MockBoxGlobalTimeWindowCounter {
        async fn get(&self, _time_window: u32) -> Result<f64, Error> {
            let count = self.count.lock().unwrap();
            Ok(*count)
        }
        fn bump(&self, value: f64) {
            let mut count = self.count.lock().unwrap();
            *count += value;
        }
    }

    #[mononoke::fbinit_test]
    async fn test_counter_check_and_bump(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let max_value = 10.0;
        let scuba_extras = HashMap::new();

        // Test case: Counter below maximum value
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(5.0)),
        });
        let result =
            counter_check_and_bump(&ctx, counter, max_value, true, scuba_extras.clone()).await;
        assert!(result.is_ok());
        // Counter equals maximum value
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(10.0)),
        });
        let result =
            counter_check_and_bump(&ctx, counter, max_value, true, scuba_extras.clone()).await;
        assert!(result.is_err());
        // Counter exceeds maximum value with enforcement
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(11.0)),
        });
        let result =
            counter_check_and_bump(&ctx, counter, max_value, true, scuba_extras.clone()).await;
        assert!(result.is_err());

        // Counter exceeds maximum value without enforcement
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(11.0)),
        });
        let result =
            counter_check_and_bump(&ctx, counter, max_value, false, scuba_extras.clone()).await;
        assert!(result.is_ok());
    }
}
