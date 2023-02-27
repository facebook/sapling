/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::stream::FusedStream;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use futures::Future;
use pin_project::pin_project;

#[pin_project]
pub struct NextStep<S, F>
where
    S: FusedStream,
    F: FnMut<(S::Item,)>,
    F::Output: Future,
{
    f: F,
    next_in_line: Option<S::Item>,
    running: Option<Pin<Box<F::Output>>>,
    #[pin]
    inner: S,
}

/// Like stream::Then, it applies a future to the output. The difference is
/// that it continues waiting for the next item in the stream WHILE the current
/// one is being processed.
impl<S, F> NextStep<S, F>
where
    S: FusedStream,
    F: FnMut<(S::Item,)>,
    F::Output: Future,
{
    pub fn new(inner: S, f: F) -> Self {
        Self {
            f,
            inner,
            next_in_line: None,
            running: None,
        }
    }
}

impl<S, F> Stream for NextStep<S, F>
where
    S: FusedStream,
    F: FnMut<(S::Item,)>,
    F::Output: Future,
{
    type Item = <F::Output as Future>::Output;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if !this.inner.is_terminated() && this.next_in_line.is_none() {
            if let Poll::Ready(Some(next_in_line)) = this.inner.as_mut().poll_next(ctx) {
                *this.next_in_line = Some(next_in_line);
            }
        }
        if this.running.is_none() {
            if let Some(next_in_line) = this.next_in_line.take() {
                let running = (this.f)(next_in_line);
                *this.running = Some(Box::pin(running));
            }
        }
        if let Some(running) = this.running.as_mut() {
            if let Poll::Ready(processed) = running.as_mut().poll(ctx) {
                *this.running = None;
                return Poll::Ready(Some(processed));
            }
        } else if this.inner.is_terminated() {
            // Nothing running, inner stream is empty, we're done
            return Poll::Ready(None);
        }

        Poll::Pending
    }
}

impl<S, F> FusedStream for NextStep<S, F>
where
    S: FusedStream,
    F: FnMut<(S::Item,)>,
    F::Output: Future,
{
    fn is_terminated(&self) -> bool {
        self.running.is_none() && self.next_in_line.is_none() && self.inner.is_terminated()
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::Relaxed;

    use futures::stream;
    use futures::StreamExt;

    use super::*;

    struct TestFuture {
        poll_count: AtomicUsize,
        value: AtomicUsize,
        set: AtomicBool,
    }
    impl Default for TestFuture {
        fn default() -> Self {
            Self {
                poll_count: 0.into(),
                value: 0.into(),
                set: false.into(),
            }
        }
    }

    impl TestFuture {
        fn poll_count(&self) -> usize {
            self.poll_count.load(Relaxed)
        }
        fn set_value(&self, x: usize) {
            self.value.store(x, Relaxed);
            self.set.store(true, Relaxed);
        }
    }

    impl Future for &TestFuture {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
            self.poll_count.fetch_add(1, Relaxed);
            if self.set.load(Relaxed) {
                Poll::Ready(self.value.load(Relaxed))
            } else {
                ctx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    #[tokio::test]
    async fn test_test_future() {
        let futs: [TestFuture; 3] = Default::default();
        let mut stream = stream::iter(&futs).buffered(1);
        assert_eq!(futs[0].poll_count(), 0);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(futs[0].poll_count(), 1);
        assert_eq!(futs[1].poll_count(), 0);
        futs[0].set_value(12);
        assert_eq!(stream.next().await, Some(12));
        assert_eq!(futs[0].poll_count(), 2);
        assert_eq!(futs[1].poll_count(), 0);
    }

    #[tokio::test]
    async fn test_stream_ext() {
        let futs: [TestFuture; 3] = Default::default();
        let then_futs: [TestFuture; 3] = Default::default();
        let mut stream = stream::iter(&futs).buffered(1).then(|x| &then_futs[x]);
        futs[0].set_value(0);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(then_futs[0].poll_count(), 2);
        assert_eq!(futs[1].poll_count(), 0);
        futs[1].set_value(1);
        futs[2].set_value(2);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        // Still waiting for then_fut[0], doesn't try fut[1]
        assert_eq!(futs[1].poll_count(), 0);
        assert_eq!(futs[2].poll_count(), 0);
    }

    #[tokio::test]
    async fn test_next_step() {
        let futs: [TestFuture; 3] = Default::default();
        let then_futs: [TestFuture; 3] = Default::default();
        let stream = stream::iter(&futs).buffered(1);
        let mut stream = NextStep::new(stream.fuse(), |x| &then_futs[x]);
        futs[0].set_value(0);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(then_futs[0].poll_count(), 2);
        assert_eq!(futs[1].poll_count(), 1);
        assert!(stream.running.is_some());
        assert!(stream.next_in_line.is_none());
        futs[1].set_value(1);
        futs[2].set_value(2);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(futures::poll!(stream.next()), Poll::Pending);
        assert_eq!(stream.next_in_line, Some(1));
        // Awaits for fut[1] while then_fut[0] is being processed
        assert_eq!(futs[1].poll_count(), 2);
        // Only keeps a single thing in queue
        assert_eq!(futs[2].poll_count(), 0);
    }
}
