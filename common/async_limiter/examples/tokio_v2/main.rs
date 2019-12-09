/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{anyhow, Error};
use chrono::Local;
use futures_util::future::join_all;
use nonzero_ext::nonzero;
use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
use std::sync::Arc;

use async_limiter::{AsyncLimiter, TokioFlavor};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let limiter = DirectRateLimiter::<LeakyBucket>::per_second(nonzero!(5u32));
    let limiter = Arc::new(AsyncLimiter::new(limiter, TokioFlavor::V02));

    let futs = (0..10).map(|i| {
        let limiter = limiter.clone();
        async move {
            loop {
                limiter.access()?.await?;
                println!("[{}] {}", i, Local::now().format("%H:%M:%S%.3f"));
            }
        }
    });

    join_all(futs)
        .await
        .into_iter()
        .collect::<Result<Vec<()>, ()>>()
        .map_err(|()| anyhow!("Error!"))?;

    Ok(())
}
