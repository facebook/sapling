/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_limiter::AsyncLimiter;
use chrono::Local;
use futures::future::join_all;
use governor::Quota;
use governor::RateLimiter;
use nonzero_ext::nonzero;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let limiter = RateLimiter::direct(Quota::per_second(nonzero!(5u32)));
    let limiter = AsyncLimiter::new(limiter).await;

    let futs = (0..10).map(|i| {
        let limiter = limiter.clone();
        async move {
            loop {
                limiter.access().await?;
                println!("[{}] {}", i, Local::now().format("%H:%M:%S%.3f"));
            }
        }
    });

    join_all(futs)
        .await
        .into_iter()
        .collect::<Result<Vec<()>, Error>>()?;

    Ok(())
}
