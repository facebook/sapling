/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::{prelude::*, sync::oneshot};

/// Given an input stream, split its error out to a separate Future, and returning that
/// error Future and an infallable Stream. There are two outcomes:
/// 1. The stream has no error - the error future never resolves
/// 2. The stream has an error - the output stream terminates, and the error future
///    resolves to the error
pub fn split_err<S: Stream>(
    s: S,
) -> (
    impl Stream<Item = S::Item, Error = !>,
    impl Future<Item = !, Error = S::Error>,
) {
    let (tx, rx) = oneshot::channel();

    (
        ErrSplitter {
            inner: s,
            err_tx: Some(tx),
        },
        ErrFuture { err_rx: Some(rx) },
    )
}

struct ErrSplitter<S: Stream> {
    inner: S,
    err_tx: Option<oneshot::Sender<S::Error>>,
}

impl<S: Stream> Stream for ErrSplitter<S> {
    type Item = S::Item;
    type Error = !;

    fn poll(&mut self) -> Poll<Option<S::Item>, !> {
        match self.inner.poll() {
            Ok(Async::Ready(v)) => Ok(Async::Ready(v)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                self.err_tx.take().map(|tx| tx.send(err));
                // If we're generating an error then this error-less stream is never going
                // to finish.
                Ok(Async::NotReady)
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
struct ErrFuture<E> {
    err_rx: Option<oneshot::Receiver<E>>,
}

impl<E> Future for ErrFuture<E> {
    type Item = !;
    type Error = E;

    fn poll(&mut self) -> Poll<!, E> {
        match self.err_rx.take() {
            None => Ok(Async::NotReady),
            Some(mut rx) => match rx.poll() {
                Ok(Async::Ready(err)) => return Err(err),
                Ok(Async::NotReady) => {
                    self.err_rx = Some(rx);
                    Ok(Async::NotReady)
                }
                Err(_) => Ok(Async::NotReady),
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::{future, stream};

    #[test]
    fn simple() {
        let vec = vec![1, 2, 3, 4, 5];
        let s = stream::iter_ok::<_, ()>(vec.clone());

        let (s, err) = split_err(s);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res: Result<Vec<_>, ()> = rt.block_on(future::lazy(move || {
            s.collect()
                .map_err(|_| -> () { unreachable!() })
                .select(err.map(|_| -> Vec<_> { unreachable!() }))
                .map(|(ok, _)| ok)
                .map_err(|(err, _)| err)
        }));

        assert_eq!(res, Ok(vec));
    }

    #[test]
    fn err() {
        let vec = vec![Ok(1), Ok(2), Ok(3), Err("badness"), Ok(5)];
        let s = stream::iter_result(vec.clone());

        let (s, err) = split_err(s);

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res: Result<Vec<_>, &str> = rt.block_on(future::lazy(move || {
            s.collect()
                .map_err(|_| -> &str { unreachable!() })
                .select(err.map(|_| -> Vec<_> { unreachable!() }))
                .map(|(ok, _)| ok)
                .map_err(|(err, _)| err)
        }));

        assert_eq!(res, Err("badness"));
    }
}
