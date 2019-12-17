/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use chrono::Local;
use futures::{
    future::{join_all, lazy, Future, IntoFuture},
    stream::{repeat, Stream},
};
use futures_ext::FutureExt as Futures01FutureExt;
use futures_util::future::{FutureExt, TryFutureExt};
use nonzero_ext::nonzero;
use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
use tokio::runtime::Runtime;

use async_limiter::{AsyncLimiter, TokioFlavor};

fn main() -> Result<(), Error> {
    let mut runtime = Runtime::new()?;

    let fut = lazy(|| {
        let limiter = DirectRateLimiter::<LeakyBucket>::per_second(nonzero!(5u32));
        let limiter = AsyncLimiter::new(limiter, TokioFlavor::V01);

        let futs = (0..10)
            .map(|i| {
                let limiter = limiter.clone();
                repeat(())
                    .and_then(move |_| match limiter.access() {
                        Ok(fut) => fut.boxed().compat().left_future(),
                        Err(e) => Err(e).into_future().right_future(),
                    })
                    .map(move |()| println!("[{}] {}", i, Local::now().format("%H:%M:%S%.3f")))
                    .for_each(|_| Ok(()))
            })
            .collect::<Vec<_>>();

        join_all(futs).map(|_| ())
    });

    runtime.block_on(fut)?;

    Ok(())
}
