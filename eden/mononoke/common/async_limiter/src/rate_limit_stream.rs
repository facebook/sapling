/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::{ready, task::Context, FutureExt, Stream};
use ratelimit_meter::{
    algorithms::{leaky_bucket::TooEarly, Algorithm},
    clock::Clock,
    example_algorithms::Impossible,
    DirectRateLimiter, NonConformance,
};
use std::pin::Pin;
use std::task::Poll;
use std::time::Instant;

#[must_use = "streams do nothing unless you poll them"]
pub struct RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
{
    limiter: DirectRateLimiter<A, C>,
    pending: Option<tokio::time::Delay>,
}

impl<A, C> RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
{
    pub fn new(limiter: DirectRateLimiter<A, C>) -> Self {
        Self {
            limiter,
            pending: None,
        }
    }
}

/// This is normally implemented automatically for us, but we don't get this here because of the
/// generic bounds on A.
impl<A, C> Unpin for RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
{
}

impl<A, C> Stream for RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
    A::NegativeDecision: EarliestPossible,
{
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(ref mut pending) = self.pending {
                let _ = ready!(pending.poll_unpin(cx));
                self.pending = None;
            }

            match self.limiter.check() {
                Ok(()) => return Poll::Ready(Some(())),
                Err(nc) => {
                    let instant = nc.earliest_possible();
                    self.pending = Some({
                        let instant = tokio::time::Instant::from_std(instant);
                        tokio::time::delay_until(instant)
                    });
                }
            }
        }
    }
}

/// We create this extension trait instead of using ratelimit_meter's NonConformance trait to
/// support algorithms from ratelimit_meter that return NonConformance as well as those that return
/// something else (e.g. Impossible).
pub trait EarliestPossible {
    fn earliest_possible(&self) -> Instant;
}

impl EarliestPossible for TooEarly<Instant> {
    fn earliest_possible(&self) -> Instant {
        <Self as NonConformance<Instant>>::earliest_possible(self)
    }
}

impl EarliestPossible for Impossible {
    fn earliest_possible(&self) -> Instant {
        Instant::now()
    }
}
