/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! An implementation of `futures_stats` for Futures 0.3.

use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use futures::TryStream;
use futures::future::Future;
use futures::future::TryFuture;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use futures_ext::future::CancelData;

use super::FutureStats;
use super::StreamStats;
use crate::TryStreamStats;

/// A Future that gathers some basic statistics for inner Future.
/// This structure's main usage is by calling [TimedFutureExt::timed].
pub struct TimedFuture<F> {
    inner: F,
    start: Option<Instant>,
    poll_count: u64,
    poll_time: Duration,
    max_poll_time: Duration,
}

impl<F> TimedFuture<F> {
    fn new(future: F) -> Self {
        TimedFuture {
            inner: future,
            start: None,
            poll_count: 0,
            poll_time: Duration::from_secs(0),
            max_poll_time: Duration::from_secs(0),
        }
    }
}

impl<F: Future> Future for TimedFuture<F> {
    type Output = (FutureStats, F::Output);

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let _ = this.start.get_or_insert_with(Instant::now);
        this.poll_count += 1;

        let poll_start = Instant::now();

        let poll = unsafe { Pin::new_unchecked(&mut this.inner).poll(cx) };
        this.poll_time += poll_start.elapsed();
        this.max_poll_time = poll_start.elapsed().max(this.max_poll_time);

        let out = match poll {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(v) => v,
        };

        let stats = FutureStats {
            completion_time: this.start.expect("start time not set").elapsed(),
            poll_time: this.poll_time,
            max_poll_time: this.max_poll_time,
            poll_count: this.poll_count,
        };

        Poll::Ready((stats, out))
    }
}

impl<F> CancelData for TimedFuture<F> {
    type Data = FutureStats;

    fn cancel_data(&self) -> Self::Data {
        FutureStats {
            completion_time: self
                .start
                .map_or_else(|| Duration::from_secs(0), |start| start.elapsed()),
            poll_time: self.poll_time,
            poll_count: self.poll_count,
            max_poll_time: self.max_poll_time,
        }
    }
}

/// A Future that gathers some basic statistics for inner TryFuture.  This structure's main usage
/// is by calling [TimedTryFutureExt::try_timed].
pub struct TimedTryFuture<F> {
    inner: TimedFuture<F>,
}

impl<F> TimedTryFuture<F> {
    fn new(future: F) -> Self {
        Self {
            inner: TimedFuture::new(future),
        }
    }
}

impl<I, E, F: Future<Output = Result<I, E>>> Future for TimedTryFuture<F> {
    type Output = Result<(FutureStats, I), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let poll = unsafe { Pin::new_unchecked(&mut this.inner).poll(cx) };

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready((stats, Ok(v))) => Poll::Ready(Ok((stats, v))),
            Poll::Ready((_, Err(e))) => Poll::Ready(Err(e)),
        }
    }
}

impl<F> CancelData for TimedTryFuture<F> {
    type Data = FutureStats;

    fn cancel_data(&self) -> Self::Data {
        self.inner.cancel_data()
    }
}

/// A Stream that gathers some basic statistics for inner Stream.
/// This structure's main usage is by calling [TimedStreamExt::timed].
pub struct TimedStream<S, C>
where
    S: Stream,
    C: FnOnce(StreamStats),
{
    inner: S,
    callback: Option<C>,
    start: Option<Instant>,
    count: usize,
    poll_count: u64,
    poll_time: Duration,
    max_poll_time: Duration,
    first_item_time: Option<Duration>,
    completed: bool,
}

impl<S, C> TimedStream<S, C>
where
    S: Stream,
    C: FnOnce(StreamStats),
{
    fn new(stream: S, callback: Option<C>) -> Self {
        TimedStream {
            inner: stream,
            callback,
            start: None,
            count: 0,
            poll_count: 0,
            poll_time: Duration::from_secs(0),
            max_poll_time: Duration::from_secs(0),
            first_item_time: None,
            completed: false,
        }
    }

    fn gen_stats(&self) -> StreamStats {
        StreamStats {
            completion_time: self.start.as_ref().map(Instant::elapsed),
            poll_time: self.poll_time,
            max_poll_time: self.max_poll_time,
            poll_count: self.poll_count,
            count: self.count,
            first_item_time: self.first_item_time,
            completed: self.completed,
        }
    }

    fn run_callback(&mut self) {
        if let Some(callback) = self.callback.take() {
            let stats = self.gen_stats();
            callback(stats)
        }
    }
}

impl<S, C> Stream for TimedStream<S, C>
where
    S: Stream,
    C: FnOnce(StreamStats),
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };

        if this.completed {
            // The stream has already been polled to completion.
            return Poll::Ready(None);
        }

        let _ = this.start.get_or_insert_with(Instant::now);
        this.poll_count += 1;

        let poll_start = Instant::now();
        let poll = unsafe { Pin::new_unchecked(&mut this.inner).poll_next(cx) };
        this.poll_time += poll_start.elapsed();
        this.max_poll_time = poll_start.elapsed().max(this.max_poll_time);
        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => {
                this.count += 1;
                if this.count == 1 {
                    this.first_item_time = Some(this.start.expect("start time not set").elapsed());
                }
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => {
                this.completed = true;
                this.run_callback();
                Poll::Ready(None)
            }
        }
    }
}

impl<S, C> Drop for TimedStream<S, C>
where
    S: Stream,
    C: FnOnce(StreamStats),
{
    fn drop(&mut self) {
        self.run_callback();
    }
}

/// A Stream that gathers some basic statistics for inner TryStream.
/// This structure's main usage is by calling [TimedTryStreamExt::try_timed].
pub struct TimedTryStream<S, C>
where
    S: TryStream + Sized,
    C: FnOnce(TryStreamStats),
{
    callback: Option<C>,
    inner: TimedStream<S, fn(StreamStats) -> ()>,
    error_count: usize,
    first_error_position: Option<usize>,
}
impl<S, C> TimedTryStream<S, C>
where
    S: TryStream,
    C: FnOnce(TryStreamStats),
{
    fn new(stream: S, callback: C) -> Self {
        TimedTryStream {
            callback: Some(callback),
            inner: TimedStream::new(stream, None),
            error_count: 0,
            first_error_position: None,
        }
    }

    fn gen_stats(&self) -> TryStreamStats {
        TryStreamStats {
            stream_stats: self.inner.gen_stats(),
            error_count: self.error_count,
            first_error_position: self.first_error_position,
        }
    }

    fn run_callback(&mut self) {
        if let Some(callback) = self.callback.take() {
            let stats = self.gen_stats();
            callback(stats)
        }
    }
}

impl<S, C, T, E> Stream for TimedTryStream<S, C>
where
    S: Stream<Item = Result<T, E>>,
    C: FnOnce(TryStreamStats),
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };

        let poll = unsafe { Pin::new_unchecked(&mut this.inner).poll_next(cx) };
        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => {
                if item.is_err() {
                    this.error_count += 1;
                    if this.first_error_position.is_none() {
                        this.first_error_position = Some(this.inner.count - 1)
                    }
                }
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => {
                this.run_callback();
                Poll::Ready(None)
            }
        }
    }
}

impl<S, C> Drop for TimedTryStream<S, C>
where
    S: TryStream,
    C: FnOnce(TryStreamStats),
{
    fn drop(&mut self) {
        self.run_callback();
    }
}

/// A trait that provides the `timed` method to [futures::Future] for gathering stats
pub trait TimedFutureExt: Future + Sized {
    /// Combinator that returns a future that will gather some statistics and
    /// return them together with the result of inner future.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_stats::TimedFutureExt;
    ///
    /// # futures::executor::block_on(async {
    /// let (stats, value) = async { 123u32 }.timed().await;
    /// assert_eq!(value, 123);
    /// assert!(stats.poll_count > 0);
    /// # });
    /// ```
    fn timed(self) -> TimedFuture<Self> {
        TimedFuture::new(self)
    }
}

impl<T: Future> TimedFutureExt for T {}

/// A trait that provides the `timed` method to [futures::Future] for gathering stats
pub trait TimedTryFutureExt: TryFuture + Sized {
    /// Combinator that returns a future that will gather some statistics and
    /// return them together with the result of inner future.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_stats::TimedTryFutureExt;
    ///
    /// # futures::executor::block_on(async {
    /// let (stats, value) = async { Result::<_, ()>::Ok(123u32) }
    ///     .try_timed()
    ///     .await
    ///     .unwrap();
    /// assert_eq!(value, 123);
    /// assert!(stats.poll_count > 0);
    /// # });
    /// ```
    fn try_timed(self) -> TimedTryFuture<Self> {
        TimedTryFuture::new(self)
    }
}

impl<T: TryFuture> TimedTryFutureExt for T {}

/// A trait that provides the `timed` method to [futures::Stream] for gathering stats
pub trait TimedStreamExt: Stream + Sized {
    /// Combinator that returns a stream that will gather some statistics and
    /// pass them for inspection to the provided callback when the stream
    /// completes.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::StreamExt;
    /// use futures::stream::{self};
    /// use futures_stats::TimedStreamExt;
    ///
    /// # futures::executor::block_on(async {
    /// let out = stream::iter([0u32; 3].iter())
    ///     .timed(|stats| {
    ///         assert_eq!(stats.count, 3);
    ///     })
    ///     .collect::<Vec<u32>>()
    ///     .await;
    /// assert_eq!(out, vec![0, 0, 0]);
    /// # });
    /// ```
    fn timed<C>(self, callback: C) -> TimedStream<Self, C>
    where
        C: FnOnce(StreamStats),
    {
        TimedStream::new(self, Some(callback))
    }
}

impl<T: Stream> TimedStreamExt for T {}

/// A trait that provides the `try_timed` method to [futures::TryStream] for gathering stats
pub trait TimedTryStreamExt: TryStream + Sized {
    /// Combinator that returns a stream that will gather some statistics and
    /// pass them for inspection to the provided callback when the stream
    /// completes.
    ///
    /// Comparered to [TimedStreamExt::timed], this method collects the stats
    /// about errors encountered in the stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::TryStreamExt;
    /// use futures::stream::{self};
    /// use futures_stats::TimedTryStreamExt;
    ///
    /// # futures::executor::block_on(async {
    /// let out = stream::iter([Ok(1), Ok(2), Err(3)])
    ///     .try_timed(|stats| {
    ///         assert_eq!(stats.error_count, 1);
    ///     })
    ///     .try_collect::<Vec<u32>>()
    ///     .await;
    /// assert!(out.is_err());
    /// # });
    /// ```
    fn try_timed<C>(self, callback: C) -> TimedTryStream<Self, C>
    where
        C: FnOnce(TryStreamStats),
    {
        TimedTryStream::new(self, callback)
    }
}

impl<T: Sized + TryStream> TimedTryStreamExt for T {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::thread;

    use futures::TryStreamExt;
    use futures::stream;
    use futures::stream::StreamExt;
    use futures_ext::FbFutureExt;

    use super::*;

    #[tokio::test]
    async fn test_timed_future() {
        let ten_millis = Duration::from_millis(10);
        let twenty_millis = Duration::from_millis(20);
        let (stats, result) = async {
            thread::sleep(ten_millis);
            tokio::task::yield_now().await;
            thread::sleep(ten_millis);
            tokio::task::yield_now().await;
            123u32
        }
        .timed()
        .await;
        assert_eq!(result, 123u32);
        assert!(stats.poll_count > 0);
        assert!(stats.poll_time > twenty_millis);
        assert!(stats.max_poll_time > ten_millis);
    }

    #[tokio::test]
    async fn test_cancel_timed_future() {
        let stats = Mutex::new(None);
        let fut = async {}
            .timed()
            .on_cancel_with_data(|data| *stats.lock().unwrap() = Some(data));
        drop(fut);
        let stats = stats.lock().unwrap();
        assert_eq!(stats.as_ref().unwrap().poll_count, 0)
    }

    #[tokio::test]
    async fn test_timed_try_future() {
        let (stats, result) = async { Result::<_, ()>::Ok(123u32) }
            .try_timed()
            .await
            .unwrap();
        assert_eq!(result, 123u32);
        assert!(stats.poll_count > 0);
    }

    #[tokio::test]
    async fn test_timed_stream() {
        let callback_called = Arc::new(AtomicBool::new(false));
        const TEST_COUNT: usize = 3;
        let out: Vec<_> = stream::iter([0u32; TEST_COUNT].iter())
            .timed({
                let callback_called = callback_called.clone();
                move |stats| {
                    assert_eq!(stats.count, TEST_COUNT);
                    assert!(stats.completed);
                    callback_called.store(true, Ordering::SeqCst);
                }
            })
            .collect::<Vec<u32>>()
            .await;
        assert_eq!(out, vec![0; TEST_COUNT]);
        assert!(callback_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_cancel_timed_stream() {
        let callback_called = Arc::new(AtomicBool::new(false));
        const TEST_COUNT: usize = 3;
        let mut s = stream::iter([0u32; TEST_COUNT].iter()).timed({
            let callback_called = callback_called.clone();
            move |stats| {
                assert_eq!(stats.count, 1);
                assert!(!stats.completed);
                callback_called.store(true, Ordering::SeqCst);
            }
        });
        let first = s.next().await;
        assert_eq!(first, Some(&0));
        assert!(!callback_called.load(Ordering::SeqCst));
        drop(s);
        assert!(callback_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_try_timed_stream() {
        let callback_called = Arc::new(AtomicBool::new(false));
        let out = stream::iter([
            Ok(0),
            Err("Integer overflow".to_owned()),
            Ok(1),
            Ok(2),
            Err("Rounding error".to_owned()),
            Err("Unit conversion error".to_owned()),
        ])
        .try_timed({
            let callback_called = callback_called.clone();
            move |stats: TryStreamStats| {
                assert_eq!(stats.stream_stats.count, 6);
                assert_eq!(stats.error_count, 3);
                assert_eq!(stats.first_error_position, Some(1));
                assert!(stats.stream_stats.completed);
                callback_called.store(true, Ordering::SeqCst);
            }
        })
        .collect::<Vec<Result<u32, _>>>()
        .await;
        assert_eq!(out[2], Ok(1));
        assert!(callback_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_cancel_try_timed_stream() {
        let callback_called = Arc::new(AtomicBool::new(false));
        let out = stream::iter([
            Ok(0),
            Err("Integer overflow".to_owned()),
            Ok(1),
            Ok(2),
            Err("Rounding error".to_owned()),
            Err("Unit conversion error".to_owned()),
        ])
        .try_timed({
            let callback_called = callback_called.clone();
            move |stats: TryStreamStats| {
                assert_eq!(stats.stream_stats.count, 2);
                assert_eq!(stats.error_count, 1);
                assert_eq!(stats.first_error_position, Some(1));
                assert!(!stats.stream_stats.completed);
                callback_called.store(true, Ordering::SeqCst);
            }
        })
        // Try collect will drop the stream after first failure
        .try_collect::<Vec<u32>>()
        .await;
        assert!(out.is_err());
        assert!(callback_called.load(Ordering::SeqCst));
    }
}
