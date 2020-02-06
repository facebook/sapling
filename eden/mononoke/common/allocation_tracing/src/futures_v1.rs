/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use futures::{Async, Future, Stream};
use std::cmp;

use crate::trace_allocations;

#[must_use = "futures and streams do nothing unless polled"]
pub struct AllocationTraced<T> {
    inner: T,
    delta: i64,
    high: i64,
}

impl<T> AllocationTraced<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            delta: 0,
            high: 0,
        }
    }
}

impl<T> AllocationTraced<T> {
    fn do_poll<I, E, P>(&mut self, poll: P) -> Result<Async<I>, E>
    where
        P: FnOnce(&mut T) -> Result<Async<I>, E>,
        E: From<Error>,
    {
        let (ret, stats) = trace_allocations(|| poll(&mut self.inner));

        let delta = match stats.delta() {
            Ok(delta) => delta,
            Err(e) => {
                return Err(e.into());
            }
        };

        let new_delta = self.delta + delta;

        self.delta = new_delta;
        self.high = cmp::max(self.high, new_delta);

        ret
    }
}

/// AllocationTraced<F> returns a Future that yields the item or error from F, along with the high
/// watermark for bytes allocated throughout the execution of F.
impl<T> Future for AllocationTraced<T>
where
    T: Future,
    T::Error: From<Error>,
{
    type Item = (T::Item, i64);
    type Error = (T::Error, i64);

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        match self.do_poll(T::poll) {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(r)) => Ok(Async::Ready((r, self.high))),
            Err(e) => Err((e, self.high)),
        }
    }
}

/// AllocationTraced<S> returns a Strem that yields items or the error from S, along with the high
/// watermark for bytes allocated throughout the execution of S.
impl<T> Stream for AllocationTraced<T>
where
    T: Stream,
    T::Error: From<Error>,
{
    type Item = (T::Item, i64);
    type Error = (T::Error, i64);

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        match self.do_poll(T::poll) {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(Some(r))) => Ok(Async::Ready(Some((r, self.high)))),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Err(e) => Err((e, self.high)),
        }
    }
}

pub trait AllocationTracingFutureExt: Future + Sized {
    fn allocation_traced(self) -> AllocationTraced<Self> {
        AllocationTraced::new(self)
    }
}

impl<T> AllocationTracingFutureExt for T where T: Future {}

pub trait AllocationTracingStreamExt: Stream + Sized {
    fn allocation_traced(self) -> AllocationTraced<Self> {
        AllocationTraced::new(self)
    }
}

impl<T> AllocationTracingStreamExt for T where T: Stream {}
