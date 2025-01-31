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
    rate_limit_name: &'a str,
    max_value: f64,
    time_window: u32,
    enforced: bool,
    scuba_extras: HashMap<&'a str, &'a str>,
) -> Result<(), Error> {
    let mut scuba = ctx.scuba().clone();
    for (key, val) in scuba_extras {
        scuba.add(key, val);
    }

    match timeout(RATELIM_FETCH_TIMEOUT, counter.get(time_window)).await {
        Ok(Ok(count)) => {
            let new_value = count + 1.0;
            if new_value <= max_value {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) does not exceed threshold {} if bumped",
                    rate_limit_name,
                    count,
                    max_value,
                );
                counter.bump(1.0);
                Ok(())
            } else if !enforced {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped, but enforcement is disabled",
                    rate_limit_name,
                    count,
                    max_value,
                );
                let msg = format!("Rate limit exceeded: {} (log only)", rate_limit_name);
                scuba.log_with_msg(&msg, None);
                Ok(())
            } else {
                debug!(
                    ctx.logger(),
                    "Rate-limiting counter {} ({}) exceeds threshold {} if bumped. Blocking request",
                    rate_limit_name,
                    count,
                    max_value,
                );
                let msg = format!("Rate limit exceeded: {} (enforced)", rate_limit_name);
                scuba.log_with_msg(&msg, None);
                Err(anyhow!(msg))
            }
        }
        Ok(Err(e)) => {
            // This can happen if the counter is not yet initialized
            // or it's not been long enough. Bump and continue.
            debug!(
                ctx.logger(),
                "Failed getting rate limiting counter {}: {:?}", rate_limit_name, e
            );
            counter.bump(1.0);
            Ok(())
        }
        Err(_) => {
            let msg = format!("{}: Timed out", rate_limit_name);
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
        let rate_limit_name = "test";
        let max_value = 10.0;
        let scuba_extras = HashMap::new();

        // Test case: Counter below maximum value
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(5.0)),
        });
        let result = counter_check_and_bump(
            &ctx,
            counter,
            rate_limit_name,
            max_value,
            1,
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
            rate_limit_name,
            max_value,
            1,
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
            rate_limit_name,
            max_value,
            1,
            true,
            scuba_extras.clone(),
        )
        .await;
        assert!(result.is_err());

        // Counter exceeds maximum value without enforcement
        let counter = Box::new(MockBoxGlobalTimeWindowCounter {
            count: Arc::new(Mutex::new(11.0)),
        });
        let result = counter_check_and_bump(
            &ctx,
            counter,
            rate_limit_name,
            max_value,
            1,
            false,
            scuba_extras.clone(),
        )
        .await;
        assert!(result.is_ok());
    }
}
