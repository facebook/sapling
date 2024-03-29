/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

#[pin_project]
pub struct YieldStream<S, F>
where
    S: Stream,
    F: Fn(&S::Item) -> usize,
{
    generated: usize,
    yield_every: usize,
    size_fn: F,
    #[pin]
    inner: S,
}

impl<S, F> YieldStream<S, F>
where
    S: Stream,
    F: Fn(&S::Item) -> usize,
{
    pub fn new(inner: S, yield_every: usize, size_fn: F) -> Self {
        Self {
            generated: 0,
            yield_every,
            inner,
            size_fn,
        }
    }
}

impl<S, F> Stream for YieldStream<S, F>
where
    S: Stream,
    F: Fn(&S::Item) -> usize,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if *this.generated >= *this.yield_every {
            *this.generated %= *this.yield_every;
            ctx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let ret = futures::ready!(this.inner.poll_next_unpin(ctx));
        if let Some(item) = &ret {
            *this.generated += (this.size_fn)(item);
        }

        Poll::Ready(ret)
    }
}

pub trait YieldStreamExt: Stream + Sized {
    /// Modify the stream to yield every time a certain amount of data is
    /// generated by the stream.
    ///
    /// The `size_fn` closure is called on each item and should return an
    /// approximation of the size of the item.
    ///
    /// Whenever the stream cumulatively produces `yield_every` values (that
    /// is, the sum of the values returned from `size_fn` exceeds
    /// `yield_every`), the stream will yield back to the executor.
    fn yield_every<F>(self, yield_every: usize, size_fn: F) -> YieldStream<Self, F>
    where
        F: Fn(&Self::Item) -> usize + Sized,
    {
        YieldStream::new(self, yield_every, size_fn)
    }
}

impl<S: Stream> YieldStreamExt for S {}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use futures::stream;

    use super::*;

    #[tokio::test]
    async fn test_yield_every() {
        // NOTE: This tests that the yield probably wakes up but assumes it yields.

        let data = &[b"foo".as_ref(), b"bar2".as_ref()];
        let data = stream::iter(
            data.iter()
                .map(|d| Result::<_, ()>::Ok(Bytes::copy_from_slice(d))),
        );
        let mut stream = data.yield_every(1, |data| data.as_ref().map_or(0, |b| b.len()));

        assert_eq!(
            stream.next().await,
            Some(Ok(Bytes::copy_from_slice(b"foo")))
        );

        assert!(stream.generated > stream.yield_every);

        assert_eq!(
            stream.next().await,
            Some(Ok(Bytes::copy_from_slice(b"bar2")))
        );

        assert_eq!(stream.next().await, None,);
    }
}
