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

/// Retry a function whenever it fails.
/// See `retry` for more information.
pub async fn retry_always<V, Fut, Func, Error>(
    logger: &Logger,
    func: Func,
    base_retry_delay_ms: u64,
    retry_num: usize,
) -> Result<(V, RetryAttemptsCount), Error>
where
    V: Send + 'static,
    Fut: Future<Output = Result<V, Error>>,
    Func: FnMut(usize) -> Fut + Send,
{
    retry(Some(logger), func, |_| true, base_retry_delay_ms, retry_num).await
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
    base_retry_delay_ms: u64,
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

                let delay = Duration::from_millis(base_retry_delay_ms * 2u64.pow(attempt as u32));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}
