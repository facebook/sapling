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

use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryStream;
use futures::future;
use futures::future::BoxFuture;
use futures::ready;
use futures::stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

/// Params for [crate::FbStreamExt::buffered_weight_limited] and [WeightLimitedBufferedStream]
#[derive(Clone, Copy, Debug)]
pub struct BufferedParams {
    /// Limit for the sum of weights in the [WeightLimitedBufferedStream] stream
    pub weight_limit: u64,
    /// Limit for size of buffer in the [WeightLimitedBufferedStream] stream
    pub buffer_size: usize,
}

/// Like [stream::Buffered], but can also limit number of futures in a buffer by "weight".
#[pin_project]
pub struct WeightLimitedBufferedStream<'a, S, I> {
    #[pin]
    queue: stream::FuturesOrdered<BoxFuture<'a, (I, u64)>>,
    current_weight: u64,
    weight_limit: u64,
    max_buffer_size: usize,
    #[pin]
    stream: stream::Fuse<S>,
}

impl<S, I> WeightLimitedBufferedStream<'_, S, I>
where
    S: Stream,
{
    /// Create a new instance that will be configured using the `params` provided
    pub fn new(params: BufferedParams, stream: S) -> Self {
        Self {
            queue: stream::FuturesOrdered::new(),
            current_weight: 0,
            weight_limit: params.weight_limit,
            max_buffer_size: params.buffer_size,
            stream: stream.fuse(),
        }
    }
}

impl<'a, S, Fut, I: 'a> Stream for WeightLimitedBufferedStream<'a, S, I>
where
    S: Stream<Item = (Fut, u64)>,
    Fut: Future<Output = I> + Send + 'a,
{
    type Item = I;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // First up, try to spawn off as many futures as possible by filling up
        // our slab of futures.
        while this.queue.len() < *this.max_buffer_size && this.current_weight < this.weight_limit {
            let future = match this.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some((f, weight))) => {
                    *this.current_weight += weight;
                    f.map(move |val| (val, weight)).boxed()
                }
                Poll::Ready(None) | Poll::Pending => break,
            };

            this.queue.push_back(future);
        }

        // Try polling a new future
        if let Some((val, weight)) = ready!(this.queue.poll_next(cx)) {
            *this.current_weight -= weight;
            return Poll::Ready(Some(val));
        }

        // If we've gotten this far, then there are no events for us to process
        // and nothing was ready, so figure out if we're not done yet or if
        // we've reached the end.
        if this.stream.is_done() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// Like [stream::Buffered], but is for TryStream and can also
/// limit number of futures in a buffer by "weight"
#[pin_project]
pub struct WeightLimitedBufferedTryStream<'a, S, I, E> {
    #[pin]
    queue: stream::FuturesOrdered<BoxFuture<'a, (Result<I, E>, u64)>>,
    current_weight: u64,
    weight_limit: u64,
    max_buffer_size: usize,
    #[pin]
    stream: stream::Fuse<S>,
}

impl<S, I, E> WeightLimitedBufferedTryStream<'_, S, I, E>
where
    S: TryStream,
{
    /// Create a new instance that will be configured using the `params` provided
    pub fn new(params: BufferedParams, stream: S) -> Self {
        Self {
            queue: stream::FuturesOrdered::new(),
            current_weight: 0,
            weight_limit: params.weight_limit,
            max_buffer_size: params.buffer_size,
            stream: stream.fuse(),
        }
    }
}

impl<'a, S, Fut, I: 'a, E> Stream for WeightLimitedBufferedTryStream<'a, S, I, E>
where
    S: Stream<Item = Result<(Fut, u64), E>>,
    Fut: Future<Output = Result<I, E>> + Send + 'a,
    E: Send + 'a,
    I: Send,
{
    type Item = Result<I, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // First up, try to spawn off as many futures as possible by filling up
        // our slab of futures.
        while this.queue.len() < *this.max_buffer_size && this.current_weight < this.weight_limit {
            let future = match this.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok((f, weight)))) => {
                    *this.current_weight += weight;
                    f.map(move |val| (val, weight)).boxed()
                }
                Poll::Ready(Some(Err(e))) => {
                    // We failed to even get the weight of the future
                    // Let's record the failure in the queue instead
                    // of returning error from the stream now. Otherwise
                    // the error returned now may actually correspond
                    // to a future for which we succeeded querying weight.
                    // Note: this behavior is different from what we had
                    //       in `WeightLimitedBufferedStream` for Stream 0.1
                    //       but IMO it's more correct, as the stream can
                    //       keep returning successes after an error
                    future::ready((Err(e), 0u64)).boxed()
                }
                Poll::Ready(None) | Poll::Pending => break,
            };

            this.queue.push_back(future);
        }

        // Try polling a new future
        if let Some((val, weight)) = ready!(this.queue.poll_next(cx)) {
            *this.current_weight -= weight;
            return Poll::Ready(Some(val));
        }

        // If we've gotten this far, then there are no events for us to process
        // and nothing was ready, so figure out if we're not done yet or if
        // we've reached the end.
        if this.stream.is_done() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use futures::FutureExt;
    use futures::StreamExt;
    use futures::future;
    use futures::future::BoxFuture;
    use futures::stream;
    use futures::stream::BoxStream;

    use super::*;

    type TestStream = BoxStream<'static, (BoxFuture<'static, ()>, u64)>;

    fn create_stream() -> (Arc<AtomicUsize>, TestStream) {
        let s: TestStream = stream::iter(vec![
            (future::ready(()).boxed(), 100),
            (future::ready(()).boxed(), 2),
            (future::ready(()).boxed(), 7),
        ])
        .boxed();

        let counter = Arc::new(AtomicUsize::new(0));

        (
            counter.clone(),
            s.inspect({
                move |_val| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })
            .boxed(),
        )
    }

    #[tokio::test]
    async fn test_too_much_weight_to_do_in_one_go() {
        let (counter, s) = create_stream();
        let params = BufferedParams {
            weight_limit: 10,
            buffer_size: 10,
        };
        let s = WeightLimitedBufferedStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(()), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 1);
                assert_eq!(s.collect::<Vec<()>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    #[tokio::test]
    async fn test_all_in_one_go() {
        let (counter, s) = create_stream();
        let params = BufferedParams {
            weight_limit: 200,
            buffer_size: 10,
        };
        let s = WeightLimitedBufferedStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(()), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 3);
                assert_eq!(s.collect::<Vec<()>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    #[tokio::test]
    async fn test_too_much_items_to_do_in_one_go() {
        let (counter, s) = create_stream();
        let params = BufferedParams {
            weight_limit: 1000,
            buffer_size: 2,
        };
        let s = WeightLimitedBufferedStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(()), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 2);
                assert_eq!(s.collect::<Vec<()>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    type Error = String;
    type TestTryStream =
        BoxStream<'static, Result<(BoxFuture<'static, Result<(), Error>>, u64), Error>>;

    fn counted_try_stream(s: TestTryStream) -> (Arc<AtomicUsize>, TestTryStream) {
        let counter = Arc::new(AtomicUsize::new(0));

        (
            counter.clone(),
            s.inspect({
                move |_val| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })
            .boxed(),
        )
    }

    fn create_try_stream_all_good() -> (Arc<AtomicUsize>, TestTryStream) {
        let s: TestTryStream = stream::iter(vec![
            Ok((future::ready(Ok(())).boxed(), 100)),
            Ok((future::ready(Ok(())).boxed(), 2)),
            Ok((future::ready(Ok(())).boxed(), 7)),
        ])
        .boxed();

        counted_try_stream(s)
    }

    #[tokio::test]
    async fn test_try_all_in_one_go() {
        let (counter, s) = create_try_stream_all_good();
        let params = BufferedParams {
            weight_limit: 200,
            buffer_size: 10,
        };
        let s = WeightLimitedBufferedTryStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(Ok(())), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 3);
                assert_eq!(s.collect::<Vec<_>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    #[tokio::test]
    async fn test_try_too_much_weight_to_do_in_one_go() {
        let (counter, s) = create_try_stream_all_good();
        let params = BufferedParams {
            weight_limit: 10,
            buffer_size: 10,
        };
        let s = WeightLimitedBufferedTryStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(Ok(())), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 1);
                assert_eq!(s.collect::<Vec<_>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    #[tokio::test]
    async fn test_try_too_much_items_to_do_in_one_go() {
        let (counter, s) = create_try_stream_all_good();
        let params = BufferedParams {
            weight_limit: 1000,
            buffer_size: 2,
        };
        let s = WeightLimitedBufferedTryStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(Ok(())), s) => {
                assert_eq!(counter.load(Ordering::SeqCst), 2);
                assert_eq!(s.collect::<Vec<_>>().await.len(), 2);
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    fn create_try_stream_fail_external() -> (Arc<AtomicUsize>, TestTryStream) {
        let s: TestTryStream = stream::iter(vec![
            Ok((future::ready(Ok(())).boxed(), 100)),
            Err("failed to calculate weight".to_string()),
            Ok((future::ready(Ok(())).boxed(), 7)),
        ])
        .boxed();

        counted_try_stream(s)
    }

    #[tokio::test]
    async fn test_try_fail_to_calculate_weight() {
        let (counter, s) = create_try_stream_fail_external();
        let params = BufferedParams {
            weight_limit: 1000,
            buffer_size: 2,
        };
        let s = WeightLimitedBufferedTryStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(Ok(())), s) => {
                // Producting the very first value caused a buffer
                // to be filled with 2 futures
                assert_eq!(counter.load(Ordering::SeqCst), 2);
                let v = s.collect::<Vec<Result<_, _>>>().await;
                // Second element of the resulting stream is an
                // error, since we could not even calculate its
                // weithg and get its future
                assert!(v[0].is_err());
                assert!(
                    v[0].clone()
                        .unwrap_err()
                        .contains("failed to calculate weight")
                );
                // Third element of the resulting stream was
                // successfully produced
                assert_eq!(v[1], Ok(()));
                assert_eq!(v.len(), 2);
                // Collecting the while resulting stream caused
                // 3 elements of the inner stream to be polled
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }

    fn create_try_stream_fail_internal() -> (Arc<AtomicUsize>, TestTryStream) {
        let s: TestTryStream = stream::iter(vec![
            Ok((future::ready(Ok(())).boxed(), 100)),
            Ok((
                future::ready(Err("failed to produce interesting value".to_string())).boxed(),
                2,
            )),
            Ok((future::ready(Ok(())).boxed(), 7)),
        ])
        .boxed();

        counted_try_stream(s)
    }

    #[tokio::test]
    async fn test_try_fail_to_calculate_inner_value() {
        let (counter, s) = create_try_stream_fail_internal();
        let params = BufferedParams {
            weight_limit: 1000,
            buffer_size: 2,
        };
        let s = WeightLimitedBufferedTryStream::new(params, s);

        match StreamExt::into_future(s).await {
            (Some(Ok(())), s) => {
                // Producting the very first value caused a buffer
                // to be filled with 2 futures
                assert_eq!(counter.load(Ordering::SeqCst), 2);
                let v = s.collect::<Vec<Result<_, _>>>().await;
                // Second element of the resulting stream is an
                // error
                assert!(v[0].is_err());
                assert!(
                    v[0].clone()
                        .unwrap_err()
                        .contains("failed to produce interesting value")
                );
                // Third element of the resulting stream was
                // successfully produced
                assert_eq!(v[1], Ok(()));
                assert_eq!(v.len(), 2);
                // Collecting the while resulting stream caused
                // 3 elements of the inner stream to be polled
                assert_eq!(counter.load(Ordering::SeqCst), 3);
            }
            _ => {
                panic!("Stream did not produce even a single value");
            }
        }
    }
}
