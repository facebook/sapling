/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

//! Module extending functionality of [`futures::stream`] module

mod return_remainder;
mod stream_with_timeout;
mod weight_limited_buffered_stream;
mod yield_periodically;

use std::time::Duration;

use futures::Future;
use futures::Stream;
use futures::StreamExt;
use futures::TryFuture;
use futures::TryStream;

pub use self::return_remainder::ReturnRemainder;
pub use self::stream_with_timeout::StreamTimeoutError;
pub use self::stream_with_timeout::StreamWithTimeout;
pub use self::weight_limited_buffered_stream::BufferedParams;
pub use self::weight_limited_buffered_stream::WeightLimitedBufferedStream;
pub use self::weight_limited_buffered_stream::WeightLimitedBufferedTryStream;
pub use self::yield_periodically::YieldPeriodically;
use crate::future::ConservativeReceiver;

/// A trait implemented by default for all Streams which extends the standard
/// functionality.
pub trait FbStreamExt: Stream {
    /// Creates a stream wrapper and a future. The future will resolve into the wrapped stream when
    /// the stream wrapper returns None. It uses ConservativeReceiver to ensure that deadlocks are
    /// easily caught when one tries to poll on the receiver before consuming the stream.
    fn return_remainder(self) -> (ReturnRemainder<Self>, ConservativeReceiver<Self>)
    where
        Self: Sized,
    {
        ReturnRemainder::new(self)
    }

    /// Like [futures::stream::StreamExt::buffered] call,
    /// but can also limit number of futures in a buffer by "weight".
    fn buffered_weight_limited<'a, I, Fut>(
        self,
        params: BufferedParams,
    ) -> WeightLimitedBufferedStream<'a, Self, I>
    where
        Self: Sized + Send + 'a,
        Self: Stream<Item = (Fut, u64)>,
        Fut: Future<Output = I>,
    {
        WeightLimitedBufferedStream::new(params, self)
    }

    /// Construct a new [self::stream_with_timeout::StreamWithTimeout].
    fn whole_stream_timeout(self, timeout: Duration) -> StreamWithTimeout<Self>
    where
        Self: Sized,
    {
        StreamWithTimeout::new(self, timeout)
    }

    /// Construct a new [self::yield_periodically::YieldPeriodically], with a sensible default.
    #[track_caller]
    fn yield_periodically<'a>(self) -> YieldPeriodically<'a, Self>
    where
        Self: Sized,
    {
        YieldPeriodically::new(self, Duration::from_millis(10))
    }
}

impl<T> FbStreamExt for T where T: Stream + ?Sized {}

/// A trait implemented by default for all TryStreams which extends the standard
/// functionality.
pub trait FbTryStreamExt: TryStream {
    /// Like [futures::stream::StreamExt::buffered] call, but for `TryStream` and
    /// can also limit number of futures in a buffer by "weight".
    fn try_buffered_weight_limited<'a, I, Fut, E>(
        self,
        params: BufferedParams,
    ) -> WeightLimitedBufferedTryStream<'a, Self, I, E>
    where
        Self: Sized + Send + 'a,
        Self: TryStream<Ok = (Fut, u64), Error = E>,
        Fut: TryFuture<Ok = I, Error = E>,
    {
        WeightLimitedBufferedTryStream::new(params, self)
    }

    /// Convert a `Stream` of `Result<Result<I, E1>, E2>` into a `Stream` of
    /// `Result<I, E1>`, assuming `E2` can convert into `E1`.
    #[allow(clippy::type_complexity)]
    fn flatten_err<I, E1, E2>(
        self,
    ) -> futures::stream::Map<Self, fn(Result<Result<I, E1>, E2>) -> Result<I, E1>>
    where
        Self: Sized,
        Self: Stream<Item = Result<Result<I, E1>, E2>>,
        E1: From<E2>,
    {
        fn flatten_err<I, E1, E2>(e: Result<Result<I, E1>, E2>) -> Result<I, E1>
        where
            E1: From<E2>,
        {
            match e {
                Ok(Ok(i)) => Ok(i),
                Ok(Err(e1)) => Err(e1),
                Err(e2) => Err(E1::from(e2)),
            }
        }

        self.map(flatten_err)
    }
}

impl<T> FbTryStreamExt for T where T: TryStream + ?Sized {}
