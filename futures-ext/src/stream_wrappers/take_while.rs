/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::{try_ready, Async, Future, IntoFuture, Poll, Stream};

use super::StreamWrapper;

/// A stream combinator which takes elements from a stream while a predicate
/// holds.
///
/// This structure is produced by the `Stream::take_while_wrapper` method.
///
/// Adapted from take_while.rs in the futures crate, with an extra StreamWrapper
/// trait defined.
#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct TakeWhile<S, P, R>
where
    S: Stream,
    R: IntoFuture,
{
    stream: S,
    pred: P,
    pending: Option<(R::Future, S::Item)>,
    done_taking: bool,
}

pub fn new<S, P, R>(s: S, p: P) -> TakeWhile<S, P, R>
where
    S: Stream,
    P: FnMut(&S::Item) -> R,
    R: IntoFuture<Item = bool, Error = S::Error>,
{
    TakeWhile {
        stream: s,
        pred: p,
        pending: None,
        done_taking: false,
    }
}

impl<S, P, R> TakeWhile<S, P, R>
where
    S: Stream,
    R: IntoFuture,
{
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

// Forwarding impl of Sink from the underlying stream
impl<S, P, R> ::futures::sink::Sink for TakeWhile<S, P, R>
where
    S: ::futures::sink::Sink + Stream,
    R: IntoFuture,
{
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    fn start_send(&mut self, item: S::SinkItem) -> ::futures::StartSend<S::SinkItem, S::SinkError> {
        self.stream.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), S::SinkError> {
        self.stream.poll_complete()
    }
}

impl<S, P, R> Stream for TakeWhile<S, P, R>
where
    S: Stream,
    P: FnMut(&S::Item) -> R,
    R: IntoFuture<Item = bool, Error = S::Error>,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<S::Item>, S::Error> {
        if self.done_taking {
            return Ok(Async::Ready(None));
        }

        if self.pending.is_none() {
            let item = match try_ready!(self.stream.poll()) {
                Some(e) => e,
                None => return Ok(Async::Ready(None)),
            };
            self.pending = Some(((self.pred)(&item).into_future(), item));
        }

        assert!(self.pending.is_some());
        match self.pending.as_mut().unwrap().0.poll() {
            Ok(Async::Ready(true)) => {
                let (_, item) = self.pending.take().unwrap();
                Ok(Async::Ready(Some(item)))
            }
            Ok(Async::Ready(false)) => {
                self.done_taking = true;
                Ok(Async::Ready(None))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(e) => {
                self.pending = None;
                Err(e)
            }
        }
    }
}

impl<S, P, R> StreamWrapper<S> for TakeWhile<S, P, R>
where
    S: Stream,
    R: IntoFuture,
{
    fn into_inner(self) -> S {
        self.stream
    }
}
