/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

/// Multiplier for the number of times the budget we overshoot before calling
/// the on_large_overshoot callback.
const BUDGET_OVERSHOOT_MULTIPLIER: u32 = 3;

/// A stream that will yield control back to the caller if it runs for more than a given duration
/// without yielding (i.e. returning Poll::Pending).  The clock starts counting the first time the
/// stream is polled, and is reset every time the stream yields.
#[pin_project]
pub struct YieldPeriodically<'a, S> {
    #[pin]
    inner: S,
    /// Default budget.
    budget: Duration,
    /// Budget left for the current iteration.
    current_budget: Duration,
    /// Whether the next iteration must yield because the budget was exceeded.
    must_yield: bool,
    /// Callback for when we overshoot the budget by more than
    /// BUDGET_OVERSHOOT_MULTIPLIER times.
    on_large_overshoot: Option<Box<dyn Fn(Duration, Duration) + Send + Sync + 'a>>,
}

impl<S> YieldPeriodically<'_, S> {
    /// Create a new [YieldPeriodically].
    pub fn new(inner: S, budget: Duration) -> Self {
        Self {
            inner,
            budget,
            current_budget: budget,
            must_yield: false,
            on_large_overshoot: None,
        }
    }

    /// Set the budget for this stream.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self.current_budget = budget;
        self
    }

    /// If we are unable to yield in time because a single poll exceeds the
    /// budget by more than BUDGET_OVERSHOOT_MULTIPLIER times, call this
    /// callback.  The caller can use this to log the location where long
    /// polls are still happening.
    pub fn on_large_overshoot<'a>(
        self,
        on_large_overshoot: impl Fn(Duration, Duration) + Send + Sync + 'a,
    ) -> YieldPeriodically<'a, S> {
        YieldPeriodically {
            on_large_overshoot: Some(Box::new(on_large_overshoot)),
            ..self
        }
    }
}

impl<S: Stream> Stream for YieldPeriodically<'_, S> {
    type Item = <S as Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if *this.must_yield {
            *this.must_yield = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let now = Instant::now();
        let res = this.inner.poll_next(cx);

        if res.is_pending() {
            *this.current_budget = *this.budget;
            return res;
        }

        let current_budget = *this.current_budget;
        let elapsed = now.elapsed();

        match this.current_budget.checked_sub(elapsed) {
            Some(new_budget) => *this.current_budget = new_budget,
            None => {
                if let Some(on_large_overshoot) = &this.on_large_overshoot {
                    if (elapsed - current_budget) > *this.budget * BUDGET_OVERSHOOT_MULTIPLIER {
                        (on_large_overshoot)(current_budget, elapsed);
                    }
                }
                *this.must_yield = true;
                *this.current_budget = *this.budget;
            }
        };

        res
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::Mutex;

    use futures::stream::StreamExt;

    use super::*;

    #[test]
    fn test_yield_happens() {
        let stream = futures::stream::repeat(()).inspect(|_| {
            // Simulate CPU work
            std::thread::sleep(Duration::from_millis(1));
        });

        let stream = YieldPeriodically::new(stream, Duration::from_millis(100));

        futures::pin_mut!(stream);

        let now = Instant::now();

        let waker = futures::task::noop_waker();
        let mut cx = futures::task::Context::from_waker(&waker);

        while stream.as_mut().poll_next(&mut cx).is_ready() {
            assert!(
                now.elapsed() < Duration::from_millis(200),
                "Stream did not yield in time"
            );
        }

        let now = Instant::now();
        let mut did_unpause = false;

        while stream.as_mut().poll_next(&mut cx).is_ready() {
            did_unpause = true;

            assert!(
                now.elapsed() < Duration::from_millis(200),
                "Stream did not yield in time"
            );
        }

        assert!(did_unpause, "Stream did not unpause");
    }

    #[tokio::test]
    async fn test_yield_registers_for_wakeup() {
        // This will hang if the stream doesn't register
        let stream = futures::stream::repeat(())
            .inspect(|_| {
                // Simulate CPU work
                std::thread::sleep(Duration::from_millis(1));
            })
            .take(30);

        let stream = YieldPeriodically::new(stream, Duration::from_millis(10));
        stream.collect::<Vec<_>>().await;
    }

    #[tokio::test]
    async fn test_on_large_overshoot() {
        let stream = futures::stream::repeat(()).inspect(|_| {
            // Simulate CPU work that takes longer than the budget
            std::thread::sleep(Duration::from_millis(250));
        });

        let large_overshoots = Arc::new(Mutex::new(Vec::new()));

        let stream =
            YieldPeriodically::new(stream, Duration::from_millis(10)).on_large_overshoot({
                let large_overshoots = large_overshoots.clone();
                Box::new(move |budget, elapsed| {
                    large_overshoots.lock().unwrap().push((budget, elapsed));
                })
            });

        futures::pin_mut!(stream);

        let now = Instant::now();

        let waker = futures::task::noop_waker();
        let mut cx = futures::task::Context::from_waker(&waker);

        assert!(stream.as_mut().poll_next(&mut cx).is_ready());
        assert!(now.elapsed() > Duration::from_millis(200));

        let large_overshoots = large_overshoots.lock().unwrap();
        assert_eq!(large_overshoots.len(), 1);
        assert_eq!(large_overshoots[0].0, Duration::from_millis(10));
        assert!(large_overshoots[0].1 > Duration::from_millis(200));
    }
}
