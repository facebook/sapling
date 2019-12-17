/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use futures::{
    channel::{mpsc, oneshot},
    future::{self, Future, FutureExt},
    stream::StreamExt,
};
use futures_util::future::TryFutureExt;
use ratelimit_meter::{algorithms::Algorithm, clock::Clock, DirectRateLimiter};

use crate::{EarliestPossible, ErrorKind, RateLimitStream, TokioFlavor};

/// A shared asynchronous rate limiter.
#[derive(Clone)]
pub struct AsyncLimiter {
    dispatch: mpsc::UnboundedSender<oneshot::Sender<()>>,
}

impl AsyncLimiter {
    pub fn new<A, C>(limiter: DirectRateLimiter<A, C>, flavor: TokioFlavor) -> Self
    where
        A: Algorithm<C::Instant> + 'static,
        C: Clock + Send + 'static,
        A::NegativeDecision: EarliestPossible,
    {
        let (dispatch, dispatch_recv) = mpsc::unbounded();
        let rate_limit = RateLimitStream::new(flavor, limiter);

        let worker =
            dispatch_recv
                .zip(rate_limit)
                .for_each(|(reply, ()): (oneshot::Sender<()>, ())| {
                    let _ = reply.send(());
                    future::ready(())
                });

        match flavor {
            TokioFlavor::V01 => {
                tokio::spawn(worker.map(Ok).boxed().compat());
            }
            TokioFlavor::V02 => {
                tokio_preview::spawn(worker.boxed());
            }
        }

        Self { dispatch }
    }

    /// access() returns a result of a future that returns once the rate limiter reports that it is
    /// OK to let one client proceed. If calling form an async fn, consider access_flat, which has
    /// a slightly simpler API. It may return an error if the runtime is shutting down.  Access is
    /// granted on a first-come first-serve basis (based on the order in which access() was
    /// called). If a caller doesnot await the future returned by access to completion, then the
    /// rate limiter's internal state will be updated nonetheless. Memory usage is proportional to
    /// the number of pending accesses. Note that this isn't an async fn so as to not capture a
    /// refernce to &self in the future returned by this method, which makes it more suitable for
    /// use in e.g. a futures 0.1 context.
    pub fn access(
        &self,
    ) -> Result<impl Future<Output = Result<(), Error>> + 'static + Send + Sync, Error> {
        let (send, recv) = oneshot::channel();
        let dispatch = self.dispatch.clone();

        Ok(async move {
            // NOTE: We do the dispatch in this future here, which effectively makes this lazy.
            // This ensures that if you create a future, but don't poll it immediately, it only
            // tries to enter the queue once it's polled.
            dispatch
                .unbounded_send(send)
                .map_err(|_| ErrorKind::RuntimeShuttingDown)?;

            recv.await.map_err(|_| ErrorKind::RuntimeShuttingDown)?;

            Ok(())
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use nonzero_ext::nonzero;
    use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
    use std::time::{Duration, Instant};
    use tokio_preview as tokio;

    #[tokio::test]
    async fn test_access_enters_queue_lazily() -> Result<(), Error> {
        let limiter = DirectRateLimiter::<LeakyBucket>::per_second(nonzero!(5u32));
        let limiter = AsyncLimiter::new(limiter, TokioFlavor::V02);

        for _ in 0..10 {
            let _ = limiter.access()?;
        }

        let now = Instant::now();
        limiter.access()?.await?;
        limiter.access()?.await?;

        assert!((Instant::now() - now) < Duration::from_millis(100));
        Ok(())
    }
}
