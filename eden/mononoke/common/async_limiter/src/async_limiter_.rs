/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future;
use futures::future::Future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use ratelimit_meter::algorithms::Algorithm;
use ratelimit_meter::clock::Clock;
use ratelimit_meter::DirectRateLimiter;

use crate::EarliestPossible;
use crate::ErrorKind;
use crate::RateLimitStream;

/// A shared asynchronous rate limiter.
#[derive(Clone)]
pub struct AsyncLimiter {
    dispatch: mpsc::UnboundedSender<oneshot::Sender<()>>,
    cancel: mpsc::Sender<()>,
}

impl AsyncLimiter {
    // NOTE: This function is async because it requires a Tokio runtme to spawn things. The best
    // way to require a Tokio runtime to be present is to just make the function async.
    pub async fn new<A, C>(limiter: DirectRateLimiter<A, C>) -> Self
    where
        A: Algorithm<C::Instant> + 'static,
        C: Clock + Send + 'static,
        A::NegativeDecision: EarliestPossible,
    {
        let (dispatch, dispatch_recv) = mpsc::unbounded();
        let (cancel, cancel_recv) = mpsc::channel(1);
        let rate_limit = RateLimitStream::new(limiter);

        let worker = dispatch_recv
            .zip(stream::select(cancel_recv, rate_limit))
            .for_each(|(reply, ()): (oneshot::Sender<()>, ())| {
                let _ = reply.send(());
                future::ready(())
            });

        tokio_shim::task::spawn(worker.boxed());

        Self { dispatch, cancel }
    }

    /// access() returns a result of a future that returns once the rate limiter reports that it is
    /// OK to let one client proceed. It may return an error if the runtime is shutting down.
    /// Access is granted on a first-come first-serve basis (based on the order in which access()
    /// was called). If a caller doesnot await the future returned by access to completion, then
    /// the rate limiter's internal state will be updated nonetheless. Memory usage is proportional
    /// to the number of pending accesses. Note that this isn't an async fn so as to not capture a
    /// refernce to &self in the future returned by this method, which makes it more suitable for
    /// use in e.g. a futures 0.1 context.
    pub fn access(&self) -> impl Future<Output = Result<(), Error>> + 'static + Send + Sync {
        let (send, recv) = oneshot::channel();
        let dispatch = self.dispatch.clone();

        async move {
            // NOTE: We do the dispatch in this future here, which effectively makes this lazy.
            // This ensures that if you create a future, but don't poll it immediately, it only
            // tries to enter the queue once it's polled.
            dispatch
                .unbounded_send(send)
                .map_err(|_| ErrorKind::RuntimeShuttingDown)?;

            recv.await.map_err(|_| ErrorKind::RuntimeShuttingDown)?;

            Ok(())
        }
    }

    /// cancel() allows a caller to return a token they didn't use. The token will be passed
    /// on to any active waiter. If there are no active waiters, the token is discarded.
    pub fn cancel(&self) {
        let _ = self.cancel.clone().try_send(());
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use nonzero_ext::nonzero;
    use ratelimit_meter::algorithms::LeakyBucket;
    use ratelimit_meter::DirectRateLimiter;
    use std::time::Duration;
    use std::time::Instant;

    #[tokio::test]
    async fn test_access_enters_queue_lazily() -> Result<(), Error> {
        let limiter = DirectRateLimiter::<LeakyBucket>::per_second(nonzero!(5u32));
        let limiter = AsyncLimiter::new(limiter).await;

        for _ in 0..10 {
            let _ = limiter.access();
        }

        let now = Instant::now();
        limiter.access().await?;
        limiter.access().await?;

        assert!(now.elapsed() < Duration::from_millis(100));
        Ok(())
    }

    #[tokio::test]
    async fn test_cancel() -> Result<(), Error> {
        let limiter = DirectRateLimiter::<LeakyBucket>::per_second(nonzero!(1u32));
        let limiter = AsyncLimiter::new(limiter).await;

        let now = Instant::now();

        for _ in 0..100 {
            limiter.access().await?;
            limiter.cancel();
        }

        assert!(now.elapsed() < Duration::from_millis(100));
        Ok(())
    }
}
