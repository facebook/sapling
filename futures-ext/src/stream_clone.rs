// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{
    future,
    prelude::*,
    stream::{Fuse, Stream},
    sync::mpsc,
    AsyncSink,
};
use std::mem;

/// Given an input Stream, return clones of that stream.
/// This requires both the item and the error to be cloneable.
/// This provides a single element of buffering - all clones
/// must consume each element before the original can make progress.
pub fn stream_clone<T: Clone + Send + 'static, E: Clone + Send + 'static>(
    s: impl Stream<Item = T, Error = E> + Send + 'static,
    copies: usize,
) -> Vec<impl Stream<Item = T, Error = E> + Send + 'static> {
    stream_clone_with_spawner(s, copies, tokio::executor::DefaultExecutor::current())
}

/// Given an input Stream, return clones of that stream.
/// This requires both the item and the error to be cloneable.
/// This provides a single element of buffering - all clones
/// must consume each element before the original can make progress.
/// This takes a `future::Executor` to spawn the copying task onto.
pub fn stream_clone_with_spawner<S>(
    stream: S,
    copies: usize,
    spawner: impl future::Executor<CloneCore<S>>,
) -> Vec<impl Stream<Item = S::Item, Error = S::Error> + Send + 'static>
where
    S: Stream + Send + 'static,
    S::Item: Clone + Send + 'static,
    S::Error: Clone + Send + 'static,
{
    let (senders, recvs): (Vec<_>, Vec<_>) = (0..copies).map(|_| mpsc::channel(1)).unzip();

    let core = CloneCore {
        inner: stream.fuse(),
        pending: false,
        senders,
    };

    spawner.execute(core).expect("Spawning core failed");

    recvs
        .into_iter()
        .map(|rx| rx.then(|v| v.unwrap()))
        .collect()
}

pub struct CloneCore<S: Stream> {
    /// Input stream
    inner: Fuse<S>,
    /// True while some sender is still accepting a result
    pending: bool,
    /// Downsteam streams
    senders: Vec<mpsc::Sender<Result<S::Item, S::Error>>>,
}

impl<S> Future for CloneCore<S>
where
    S: Stream,
    S::Item: Clone,
    S::Error: Clone,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            if !self.pending {
                // Initial state - we need to get a new value from the input, and all senders
                // are ready for it.
                let val = match self.inner.poll() {
                    Ok(Async::Ready(Some(val))) => Ok(val),
                    Ok(Async::Ready(None)) => break Ok(Async::Ready(())),
                    Ok(Async::NotReady) => break Ok(Async::NotReady),
                    Err(err) => Err(err),
                };

                let senders: Result<Vec<_>, _> = mem::replace(&mut self.senders, Vec::new())
                    .into_iter()
                    .filter_map(|mut tx| {
                        // Try sending. If the channel isn't ready then it (probably) means the
                        // receiver has gone away so just drop it.
                        match tx.start_send(val.clone()) {
                            Err(err) => Some(Err(err)),
                            Ok(AsyncSink::Ready) => Some(Ok(tx)),
                            Ok(AsyncSink::NotReady(_)) => None,
                        }
                    })
                    .collect();

                self.senders = senders.expect("start_send failed unexpectedly");
                self.pending = !self.senders.is_empty();
            }

            if self.pending {
                // Drive sends to completion
                let mut done = true;

                for tx in &mut self.senders {
                    match tx.poll_complete() {
                        Err(_) => return Err(()),
                        Ok(Async::Ready(())) => (),
                        Ok(Async::NotReady) => {
                            done = false;
                        }
                    }
                }

                self.pending = !done;
            }

            // If we've lost all our senders then we're done
            if self.senders.is_empty() {
                break Ok(Async::Ready(()));
            }

            // If we've still got incomplete senders, then break out
            if self.pending {
                break Ok(Async::NotReady);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::{future, stream};

    #[test]
    fn simple() {
        let vec = vec![1, 2, 3, 4, 6];
        let s = stream::iter_ok::<_, ()>(vec.clone());

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res = rt.block_on(future::lazy(|| {
            let c = stream_clone(s, 5);
            let c = c.into_iter().map(|c| c.collect());
            let c = future::join_all(c);

            c
        }));

        for (idx, v) in res.unwrap().into_iter().enumerate() {
            assert_eq!(v, vec, "idx {} mismatch", idx);
        }
    }

    #[test]
    fn err() {
        let vec = vec![Ok(1), Ok(2), Ok(3), Err("badness"), Ok(4)];
        let s = stream::iter_result(vec.clone());

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        let res: Result<_, ()> = rt.block_on(future::lazy(|| {
            let c = stream_clone(s, 5);
            let c = c.into_iter().map(|c| c.then(Result::Ok).collect());
            let c = future::join_all(c);

            c
        }));

        // Fuse keeps going after errors, so we get the entire vector.
        for (idx, v) in res.unwrap().into_iter().enumerate() {
            assert_eq!(v, vec, "idx {} mismatch", idx);
        }
    }

    // TODO some test with blocking consumers
}
