/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::pin::Pin;
use std::time::Duration;

use futures::future::Future;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use thiserror::Error;
use tokio::time::Sleep;

/// Error returned when a StreamWithTimeout exceeds its deadline.
#[derive(Debug, Error)]
#[error("Stream timeout with duration {:?} was exceeded", .0)]
pub struct StreamTimeoutError(Duration);

/// A stream that must finish within a given duration, or it will error during poll (i.e. it must
/// yield None). The clock starts counting the first time the stream is polled.
#[pin_project]
pub struct StreamWithTimeout<S> {
    #[pin]
    inner: S,
    duration: Duration,
    done: bool,
    #[pin]
    deadline: Option<Sleep>,
}

impl<S> StreamWithTimeout<S> {
    /// Create a new [StreamWithTimeout].
    pub fn new(inner: S, duration: Duration) -> Self {
        Self {
            inner,
            duration,
            done: false,
            deadline: None,
        }
    }
}

impl<S: Stream> Stream for StreamWithTimeout<S> {
    type Item = Result<<S as Stream>::Item, StreamTimeoutError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if *this.done {
            return Poll::Ready(None);
        }

        let duration = *this.duration;

        if this.deadline.is_none() {
            this.deadline.set(Some(tokio::time::sleep(duration)));
        }

        // NOTE: This unwrap() is safe as we just set the value.
        match this.deadline.as_pin_mut().unwrap().poll(cx) {
            Poll::Ready(()) => {
                *this.done = true;
                return Poll::Ready(Some(Err(StreamTimeoutError(duration))));
            }
            Poll::Pending => {
                // Continue
            }
        }

        // Keep track of whether the stream has finished, so that we don't attempt to poll the
        // deadline later if the stream has indeed finished already.
        let res = futures::ready!(this.inner.poll_next(cx));
        if res.is_none() {
            *this.done = true;
        }

        Poll::Ready(Ok(res).transpose())
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use futures::stream::StreamExt;
    use futures::stream::TryStreamExt;

    use super::*;

    #[tokio::test]
    async fn test_stream_timeout() -> Result<(), Error> {
        tokio::time::pause();

        let s = async_stream::stream! {
            yield Result::<(), Error>::Ok(());
            tokio::time::advance(Duration::from_secs(2)).await;
            yield Result::<(), Error>::Ok(());
        };

        let mut s = StreamWithTimeout::new(s.boxed(), Duration::from_secs(1)).boxed();

        assert!(s.try_next().await?.is_some());
        assert!(s.try_next().await.is_err());
        assert!(s.try_next().await?.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_stream_done_before_timeout() -> Result<(), Error> {
        tokio::time::pause();

        let s = async_stream::stream! {
            yield Result::<(), Error>::Ok(());
            yield Result::<(), Error>::Ok(());
        };

        let mut s = StreamWithTimeout::new(s.boxed(), Duration::from_secs(1)).boxed();

        assert!(s.try_next().await?.is_some());
        assert!(s.try_next().await?.is_some());
        assert!(s.try_next().await?.is_none());

        tokio::time::advance(Duration::from_secs(2)).await;

        assert!(s.try_next().await?.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_clock_starts_at_poll() -> Result<(), Error> {
        tokio::time::pause();

        let s = async_stream::stream! {
            yield Result::<(), Error>::Ok(());
            yield Result::<(), Error>::Ok(());
        };
        let mut s = StreamWithTimeout::new(s.boxed(), Duration::from_secs(1)).boxed();

        tokio::time::advance(Duration::from_secs(2)).await;

        assert!(s.try_next().await?.is_some());
        assert!(s.try_next().await?.is_some());
        assert!(s.try_next().await?.is_none());

        Ok(())
    }
}
