/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use futures::Future;
use slog::info;
use slog::Logger;

#[derive(Copy, Clone)]
pub struct RetryAttemptsCount(pub usize);

pub enum RetryLogic {
    /// Multiply by a factor every time
    Exponential { base: Duration, factor: f64 },
    /// Increase every time, and randomly jitter some more
    ExponentialWithJitter {
        base: Duration,
        factor: f64,
        jitter: Duration,
    },
}

impl RetryLogic {
    fn delay(&self, attempt: usize) -> Duration {
        use RetryLogic::*;
        match self {
            Exponential { base, factor } => base.mul_f64(factor.powf(attempt as f64)),
            ExponentialWithJitter {
                base,
                factor,
                jitter,
            } => base.mul_f64(factor.powf(attempt as f64)) + jitter.mul_f64(rand::random::<f64>()),
        }
    }
}

/// Retry a function whenever it fails.
/// See `retry` for more information.
pub async fn retry_always<V, Fut, Func, Error>(
    logger: &Logger,
    func: Func,
    base_delay_ms: u64,
    retry_num: usize,
) -> Result<(V, RetryAttemptsCount), Error>
where
    V: Send + 'static,
    Fut: Future<Output = Result<V, Error>>,
    Func: FnMut(usize) -> Fut + Send,
{
    retry(
        Some(logger),
        func,
        |_| true,
        RetryLogic::Exponential {
            base: Duration::from_millis(base_delay_ms),
            factor: 2.0,
        },
        retry_num,
    )
    .await
}

/// Retry a function.
/// `func` is the function to be retried.
/// `should_retry` tells whether an error should be retried.
/// `retry_num` is the maximum amount of times it will be retried
/// `base_retry_delay_ms` is how much to wait between retries. It does exponential backoffs, doubling this value every time.
pub async fn retry<V, Fut, Func, RetryFunc, Error>(
    logger: Option<&Logger>,
    // Function to be retried.
    mut func: Func,
    // Function that tells whether an error should be retried.
    mut should_retry: RetryFunc,
    retry_logic: RetryLogic,
    retry_num: usize,
) -> Result<(V, RetryAttemptsCount), Error>
where
    V: Send + 'static,
    Fut: Future<Output = Result<V, Error>>,
    Func: FnMut(usize) -> Fut + Send,
    RetryFunc: FnMut(&Error) -> bool + Send,
{
    let mut attempt = 1;
    loop {
        let res = func(attempt).await;
        match res {
            Ok(res) => {
                return Ok((res, RetryAttemptsCount(attempt)));
            }
            Err(err) if attempt < retry_num && should_retry(&err) => {
                if let Some(logger) = logger {
                    info!(
                        logger,
                        "retrying attempt {} of {}...",
                        attempt + 1,
                        retry_num
                    );
                }

                tokio::time::sleep(retry_logic.delay(attempt)).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

#[test]
fn test_exponential() {
    let logic = RetryLogic::Exponential {
        base: Duration::from_secs(1),
        factor: 2.0,
    };
    assert_eq!(logic.delay(0), Duration::from_secs(1));
    assert_eq!(logic.delay(1), Duration::from_secs(2));
    assert_eq!(logic.delay(2), Duration::from_secs(4));
    let logic = RetryLogic::Exponential {
        base: Duration::from_secs(8),
        factor: 1.5,
    };
    assert_eq!(logic.delay(1), Duration::from_secs(12));
    assert_eq!(logic.delay(2), Duration::from_secs(18));
}

#[test]
fn test_exponential_jitter() {
    let half_sec = Duration::from_millis(500);
    let one_sec = Duration::from_secs(1);
    let two_sec = Duration::from_secs(2);
    let logic = RetryLogic::ExponentialWithJitter {
        base: one_sec.clone(),
        factor: 2.0,
        jitter: half_sec.clone(),
    };
    let d = logic.delay(0);
    assert!(d >= one_sec && d <= one_sec + half_sec);
    let d = logic.delay(1);
    assert!(d >= two_sec && d <= two_sec + half_sec);
}
