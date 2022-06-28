/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::ready;
use futures::task::Context;
use futures::Future;
use futures::Stream;
use pin_project::pin_project;
use ratelimit_meter::algorithms::leaky_bucket::TooEarly;
use ratelimit_meter::algorithms::Algorithm;
use ratelimit_meter::clock::Clock;
use ratelimit_meter::example_algorithms::Impossible;
use ratelimit_meter::DirectRateLimiter;
use ratelimit_meter::NonConformance;
use std::pin::Pin;
use std::task::Poll;
use std::time::Instant;
use tokio_shim::time::Sleep;

#[pin_project]
#[must_use = "streams do nothing unless you poll them"]
pub struct RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
{
    limiter: DirectRateLimiter<A, C>,
    #[pin]
    pending: Option<Sleep>,
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

impl<A, C> Stream for RateLimitStream<A, C>
where
    A: Algorithm<C::Instant> + 'static,
    C: Clock + Send + 'static,
    A::NegativeDecision: EarliestPossible,
{
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            if let Some(ref mut pending) = this.pending.as_mut().as_pin_mut() {
                let _ = ready!(pending.as_mut().poll(cx));
                this.pending.set(None);
            }

            match this.limiter.check() {
                Ok(()) => return Poll::Ready(Some(())),
                Err(nc) => {
                    let instant = nc.earliest_possible();
                    this.pending
                        .set(Some(tokio_shim::time::sleep_until(instant)));
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
