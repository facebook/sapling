/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use context::CoreContext;
use rate_limiting::RateLimit;
use sha2::Digest;
use sha2::Sha256;
use slog::debug;
use time_window_counter::BoxGlobalTimeWindowCounter;
use time_window_counter::GlobalTimeWindowCounterBuilder;
use tokio::time::timeout;

const TIME_WINDOW_MIN: u32 = 1;
const TIME_WINDOW_MAX: u32 = 3600;
const RATELIM_FETCH_TIMEOUT: Duration = Duration::from_secs(1);

pub fn build_counter(
    ctx: &CoreContext,
    category: &str,
    rate_limit_name: &str,
    identifier: &str,
) -> BoxGlobalTimeWindowCounter {
    let key = make_key(rate_limit_name, identifier);
    debug!(
        ctx.logger(),
        "Associating key {:?} with client_id {:?}", key, identifier
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
pub async fn counter_check_and_bump<'a>(
    ctx: &'a CoreContext,
    counter: BoxGlobalTimeWindowCounter,
    bump_value: f64,
    rate_limit: RateLimit,
    enforced: bool,
    scuba_extras: HashMap<&'a str, &'a str>,
) -> Result<(), Error> {
    let max_value = rate_limit.body.raw_config.limit;
    let time_window = rate_limit.fci_metric.window.as_secs() as u32;

    let mut scuba = ctx.scuba().clone();
    for (key, val) in scuba_extras {
        scuba.add(key, val);
    }

    match timeout(RATELIM_FETCH_TIMEOUT, counter.get(time_window)).await {
        Ok(Ok(count)) => {
            let new_value = count + bump_value;
            if new_value <= max_value {
                counter.bump(bump_value);
                Ok(())
            } else if !enforced {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {:?} (current value:  {}) exceeds threshold {} if bumped, but enforcement is disabled",
                    rate_limit,
                    count,
                    max_value,
                );
                let log_tag = "Request would have been rejected due to rate limiting, but enforcement is disabled";
                let msg = format!("Rate limit exceeded: {:?} (log only)", rate_limit);
                scuba.log_with_msg(log_tag, msg);
                Ok(())
            } else {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {:?} (current_value: {}) exceeds threshold {} if bumped. Blocking request",
                    rate_limit,
                    count,
                    max_value,
                );
                let log_tag = "Request rejected due to rate limiting";
                let msg = format!("Rate limit exceeded: {:?} (enforced)", rate_limit);
                scuba.log_with_msg(log_tag, msg.clone());
                Err(anyhow!(msg))
            }
        }
        Ok(Err(e)) => {
            // This can happen if the counter is not yet initialized
            // or it's not been long enough. Bump and continue.
            debug!(
                ctx.logger(),
                "Failed getting rate limiting counter {:?}: {}", rate_limit, e
            );
            counter.bump(bump_value);
            Ok(())
        }
        Err(_) => {
            let log_tag = "Rate limiting counter fetch timed out";
            let msg = format!("Rate limit {:?}: Timed out", rate_limit);
            scuba.log_with_msg(log_tag, Some(msg));
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
    use rate_limiting::FciMetric;
    use rate_limiting::Metric;
    use rate_limiting::RateLimitBody;
    use rate_limiting::RateLimitStatus;
    use rate_limiting::Scope;
    use rate_limiting::Target;
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

        let limit = RateLimit {
            body: RateLimitBody {
                raw_config: rate_limiting_config::RateLimitBody {
                    limit: max_value,
                    status: RateLimitStatus::Enforced,
                },
            },
            fci_metric: FciMetric {
                metric: Metric::Commits,
                window: Duration::from_secs(1),
                scope: Scope::Global,
            },
            target: Some(Target::MainClientId("test_target".to_string())),
        };

        // Test case: Counter below maximum value
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(5.0)),
        });
        let result = counter_check_and_bump(
            &ctx,
            counter,
            1.0,
            limit.clone(),
            true,
            scuba_extras.clone(),
        )
        .await;
        assert!(result.is_ok());
        // Counter equals maximum value
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(10.0)),
        });
        let result = counter_check_and_bump(
            &ctx,
            counter,
            1.0,
            limit.clone(),
            true,
            scuba_extras.clone(),
        )
        .await;
        assert!(result.is_err());
        // Counter exceeds maximum value with enforcement
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(11.0)),
        });
        let result = counter_check_and_bump(
            &ctx,
            counter,
            1.0,
            limit.clone(),
            true,
            scuba_extras.clone(),
        )
        .await;
        assert!(result.is_err());

        // Counter exceeds maximum value without enforcement
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(11.0)),
        });
        let result =
            counter_check_and_bump(&ctx, counter, 1.0, limit, false, scuba_extras.clone()).await;
        assert!(result.is_ok());
    }
}
