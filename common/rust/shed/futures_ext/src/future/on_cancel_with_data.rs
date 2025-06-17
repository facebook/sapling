/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::pin::Pin;

use futures::future::Future;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use pin_project::pinned_drop;

/// Trait to be implemented by futures that wish to provide additional data
/// when they are canceled.
pub trait CancelData {
    /// The type of the data provided when the future is canceled.
    type Data;

    /// Provide cancellation data for this future.
    fn cancel_data(&self) -> Self::Data;
}

/// Future combinator that executes the `on_cancel` closure if the inner future
/// is canceled (dropped before completion).
#[pin_project(PinnedDrop)]
pub struct OnCancelWithData<Fut, OnCancelFn>
where
    Fut: Future + CancelData,
    OnCancelFn: FnOnce(Fut::Data),
{
    #[pin]
    inner: Fut,

    on_cancel: Option<OnCancelFn>,
}

impl<Fut, OnCancelFn> OnCancelWithData<Fut, OnCancelFn>
where
    Fut: Future + CancelData,
    OnCancelFn: FnOnce(Fut::Data),
{
    /// Construct an `OnCancelWithData` combinator that will run `on_cancel` if `inner`
    /// is canceled.  Additional data will be extracted from `inner` and
    /// passed to `on_cancel`.
    pub fn new(inner: Fut, on_cancel: OnCancelFn) -> Self {
        Self {
            inner,
            on_cancel: Some(on_cancel),
        }
    }
}

impl<Fut, OnCancelFn> Future for OnCancelWithData<Fut, OnCancelFn>
where
    Fut: Future + CancelData,
    OnCancelFn: FnOnce(Fut::Data),
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let v = ready!(this.inner.poll(cx));
        *this.on_cancel = None;
        Poll::Ready(v)
    }
}

#[pinned_drop]
impl<Fut, OnCancelFn> PinnedDrop for OnCancelWithData<Fut, OnCancelFn>
where
    Fut: Future + CancelData,
    OnCancelFn: FnOnce(Fut::Data),
{
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        if let Some(on_cancel) = this.on_cancel.take() {
            let data = this.inner.as_ref().get_ref().cancel_data();
            on_cancel(data)
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::*;

    struct WithCancelData {
        result: usize,
        data: usize,
    }

    impl Future for WithCancelData {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(self.as_ref().get_ref().result)
        }
    }

    impl CancelData for WithCancelData {
        type Data = usize;

        fn cancel_data(&self) -> Self::Data {
            self.data
        }
    }

    #[tokio::test]
    async fn runs_when_canceled() {
        let canceled = AtomicUsize::new(0);
        let fut = WithCancelData {
            result: 100,
            data: 200,
        };
        let fut = OnCancelWithData::new(fut, |data| canceled.store(data, Ordering::Relaxed));
        drop(fut);
        assert_eq!(canceled.load(Ordering::Relaxed), 200);
    }

    #[tokio::test]
    async fn doesnt_run_when_complete() {
        let canceled = AtomicUsize::new(0);
        let fut = WithCancelData {
            result: 100,
            data: 200,
        };
        let fut = OnCancelWithData::new(fut, |data| canceled.store(data, Ordering::Relaxed));
        let val = fut.await;
        assert_eq!(val, 100);
        assert_eq!(canceled.load(Ordering::Relaxed), 0);
    }
}
