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
use futures::stream::StreamExt;
use governor::clock::ReasonablyRealtime;
use governor::state::direct::DirectStateStore;
use governor::state::direct::NotKeyed;
use governor::state::direct::StreamRateLimitExt;
use governor::RateLimiter;

use crate::ErrorKind;

/// A shared asynchronous rate limiter.
#[derive(Clone)]
pub struct AsyncLimiter {
    dispatch: mpsc::UnboundedSender<oneshot::Sender<()>>,
    cancel: mpsc::Sender<()>,
}

impl AsyncLimiter {
    // NOTE: This function is async because it requires a Tokio runtme to spawn things. The best
    // way to require a Tokio runtime to be present is to just make the function async.
    pub async fn new<S, C>(limiter: RateLimiter<NotKeyed, S, C>) -> Self
    where
        S: DirectStateStore + Send + Sync + 'static,
        C: ReasonablyRealtime + Send + Sync + 'static,
    {
        let (dispatch, dispatch_recv) = mpsc::unbounded();
        let (cancel, cancel_recv) = mpsc::channel(1);

        tokio_shim::task::spawn(async move {
            let worker = dispatch_recv
                .zip(futures::stream::select(
                    cancel_recv,
                    futures::stream::repeat(()).ratelimit_stream(&limiter),
                ))
                .for_each(|(reply, _): (oneshot::Sender<()>, _)| {
                    let _ = reply.send(());
                    future::ready(())
                });
            worker.await
        });

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
    use std::time::Duration;
    use std::time::Instant;

    use governor::Quota;
    use governor::RateLimiter;
    use nonzero_ext::nonzero;

    use super::*;

    #[tokio::test]
    async fn test_access_enters_queue_lazily() -> Result<(), Error> {
        let limiter = RateLimiter::direct(Quota::per_second(nonzero!(5u32)));
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
        let limiter = RateLimiter::direct(Quota::per_second(nonzero!(1u32)));
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
