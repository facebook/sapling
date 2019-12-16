/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::{compat::Compat01As03, ready, task::Context, Stream};
use futures_util::{compat::Future01CompatExt, FutureExt};
use ratelimit_meter::{algorithms::Algorithm, DirectRateLimiter, NonConformance};
use std::pin::Pin;
use std::task::Poll;
use std::time::Instant;

use crate::TokioFlavor;

enum TokioDelay {
    V01(Compat01As03<tokio::timer::Delay>),
    V02(tokio_preview::time::Delay),
}

#[must_use = "streams do nothing unless you poll them"]
pub struct RateLimitStream<A>
where
    A: Algorithm<Instant>,
{
    flavor: TokioFlavor,
    limiter: DirectRateLimiter<A>,
    pending: Option<TokioDelay>,
}

impl<A> RateLimitStream<A>
where
    A: Algorithm<Instant>,
{
    pub fn new(flavor: TokioFlavor, limiter: DirectRateLimiter<A>) -> Self {
        Self {
            flavor,
            limiter,
            pending: None,
        }
    }
}

/// This is normally implemented automatically for us, but we don't get this here because of the
/// generic bounds on A.
impl<A: Algorithm<Instant>> Unpin for RateLimitStream<A> {}

impl<A> Stream for RateLimitStream<A>
where
    A: Algorithm<Instant>,
    A::NegativeDecision: NonConformance,
{
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(ref mut pending) = self.pending {
                match pending {
                    TokioDelay::V01(p) => {
                        let _ = ready!(p.poll_unpin(cx));
                    }
                    TokioDelay::V02(p) => {
                        let _ = ready!(p.poll_unpin(cx));
                    }
                };

                self.pending = None;
            }

            match self.limiter.check() {
                Ok(()) => return Poll::Ready(Some(())),
                Err(nc) => {
                    let instant = nc.earliest_possible();
                    self.pending = Some(match self.flavor {
                        TokioFlavor::V01 => {
                            let delay = tokio::timer::Delay::new(instant).compat();
                            TokioDelay::V01(delay)
                        }
                        TokioFlavor::V02 => {
                            let instant = tokio_preview::time::Instant::from_std(instant);
                            let delay = tokio_preview::time::delay_until(instant);
                            TokioDelay::V02(delay)
                        }
                    });
                }
            }
        }
    }
}
