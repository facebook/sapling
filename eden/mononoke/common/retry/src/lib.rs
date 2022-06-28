/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::Future;
use slog::info;
use slog::Logger;
use std::time::Duration;

#[derive(Copy, Clone)]
pub struct RetryAttemptsCount(pub usize);

pub async fn retry<V, Fut, Func, Error>(
    logger: &Logger,
    mut func: Func,
    base_retry_delay_ms: u64,
    retry_num: usize,
) -> Result<(V, RetryAttemptsCount), Error>
where
    V: Send + 'static,
    Fut: Future<Output = Result<V, Error>>,
    Func: FnMut(usize) -> Fut + Send,
{
    let mut attempt = 1;
    loop {
        let res = func(attempt).await;
        match res {
            Ok(res) => {
                return Ok((res, RetryAttemptsCount(attempt)));
            }
            Err(err) => {
                if attempt >= retry_num {
                    return Err(err);
                }
                info!(
                    logger,
                    "retrying attempt {} of {}...",
                    attempt + 1,
                    retry_num
                );

                let delay = Duration::from_millis(base_retry_delay_ms * 2u64.pow(attempt as u32));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}
