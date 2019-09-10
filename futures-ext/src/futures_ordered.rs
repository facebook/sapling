// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of the FuturesOrdered combinator, executing each future in a sequence serially
//! and streaming their results.

use std::fmt;

use futures::{Async, Future, IntoFuture, Poll, Stream};

/// A future which takes a list of futures, executes them serially, and
/// resolves with a vector of the completed values.
///
/// This future is created with the `futures_ordered` method.
#[must_use = "streams do nothing unless polled"]
pub struct FuturesOrdered<I>
where
    I: IntoIterator,
    I::Item: IntoFuture,
{
    elems: I::IntoIter,
    current: Option<<I::Item as IntoFuture>::Future>,
}

impl<I> fmt::Debug for FuturesOrdered<I>
where
    I: IntoIterator,
    I::Item: IntoFuture,
    <<I as IntoIterator>::Item as IntoFuture>::Future: fmt::Debug,
    <<I as IntoIterator>::Item as IntoFuture>::Item: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("FuturesOrdered")
            .field("current", &self.current)
            .finish()
    }
}

/// Creates a stream which returns results of the futures given.
///
/// The returned stream will serially drive execution for all of its underlying
/// futures. Errors from a future will be returned immediately, but the stream
/// will still be valid and
pub fn futures_ordered<I>(iter: I) -> FuturesOrdered<I>
where
    I: IntoIterator,
    I::Item: IntoFuture,
{
    let mut elems = iter.into_iter();
    let current = next_future(&mut elems);
    FuturesOrdered { elems, current }
}

impl<I> Stream for FuturesOrdered<I>
where
    I: IntoIterator,
    I::Item: IntoFuture,
{
    type Item = <I::Item as IntoFuture>::Item;
    type Error = <I::Item as IntoFuture>::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.current.take() {
            Some(mut fut) => {
                match fut.poll() {
                    Ok(Async::Ready(v)) => {
                        self.current = next_future(&mut self.elems);
                        Ok(Async::Ready(Some(v)))
                    }
                    Ok(Async::NotReady) => {
                        self.current = Some(fut);
                        Ok(Async::NotReady)
                    }
                    Err(e) => {
                        // Don't dump self.elems at this point because the
                        // caller might want to keep going on.
                        self.current = next_future(&mut self.elems);
                        Err(e)
                    }
                }
            }
            None => {
                // End of stream.
                Ok(Async::Ready(None))
            }
        }
    }
}

#[inline]
fn next_future<I>(elems: &mut I) -> Option<<I::Item as IntoFuture>::Future>
where
    I: Iterator,
    I::Item: IntoFuture,
{
    elems.next().map(IntoFuture::into_future)
}

#[cfg(test)]
mod test {
    use std::result;

    use futures::sync::mpsc;
    use futures::task;
    use futures::{Future, Sink, Stream};
    use tokio;

    use super::*;

    #[test]
    fn test_basic() {
        let into_futs = vec![ok(1), ok(2)];
        assert_eq!(futures_ordered(into_futs).collect().wait(), Ok(vec![1, 2]));

        let into_futs = vec![ok(1), err(2), ok(3)];
        assert_eq!(futures_ordered(into_futs).collect().wait(), Err(2));
    }

    #[test]
    fn test_serial() {
        let (tx, rx) = mpsc::channel(2);
        // If both the futures returned in parallel, rx would have [20, 10].
        // Note we move tx in. Once all tx handles have been dropped, the rx
        // stream ends.
        let futs = vec![delayed_future(10, tx.clone(), 4), delayed_future(20, tx, 2)];

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(futures_ordered(futs).collect()).unwrap();
        let results = runtime.block_on(rx.collect());
        assert_eq!(results, Ok(vec![10, 20]));
    }

    fn delayed_future<T>(v: T, tx: mpsc::Sender<T>, count: usize) -> DelayedFuture<T> {
        DelayedFuture {
            send: Some((v, tx)),
            count,
        }
    }
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    struct DelayedFuture<T> {
        send: Option<(T, mpsc::Sender<T>)>,
        count: usize,
    }

    impl<T> Future for DelayedFuture<T> {
        type Item = ();
        type Error = !;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.count -= 1;
            if self.count == 0 {
                let (v, tx) = self.send.take().unwrap();
                // In production code tx.send(v) would return a future which we could forward the
                // poll call to. In test code, this is fine.
                tx.send(v).wait().unwrap();
                Ok(Async::Ready(()))
            } else {
                // Make sure the computation moves forward.
                task::current().notify();
                Ok(Async::NotReady)
            }
        }
    }

    fn ok(v: i32) -> result::Result<i32, i32> {
        Ok(v)
    }

    fn err(v: i32) -> result::Result<i32, i32> {
        Err(v)
    }
}
