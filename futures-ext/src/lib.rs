// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

#[cfg(test)]
#[macro_use]
extern crate assert_matches;
extern crate bytes;
#[macro_use]
extern crate futures;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate tokio_core;
extern crate tokio_io;

use bytes::Bytes;
use futures::{Async, Future, IntoFuture, Poll, Sink, Stream};
use tokio_io::AsyncRead;
use tokio_io::codec::{Decoder, Encoder};

mod frame;
mod futures_ordered;
mod streamfork;
mod stream_wrappers;

pub mod decode;
pub mod encode;

pub use frame::{FramedStream, ReadLeadingBuffer};
pub use futures_ordered::{futures_ordered, FuturesOrdered};
pub use stream_wrappers::{BoxStreamWrapper, StreamWrapper, TakeWhile};

/// Map `Item` and `Error` to `()`
///
/// Adapt an existing `Future` to return unit `Item` and `Error`, while still
/// waiting for the underlying `Future` to complete.
pub struct Discard<F>(F);

impl<F> Discard<F> {
    pub fn new(f: F) -> Self {
        Discard(f)
    }
}

impl<F> Future for Discard<F>
where
    F: Future,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.0.poll() {
            Err(_) => Err(()),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(_)) => Ok(Async::Ready(())),
        }
    }
}

// Replacements for BoxFuture and BoxStream, deprecated in upstream futures-rs.
pub type BoxFuture<T, E> = Box<Future<Item = T, Error = E> + Send>;
pub type BoxFutureNonSend<T, E> = Box<Future<Item = T, Error = E>>;
pub type BoxStream<T, E> = Box<Stream<Item = T, Error = E> + Send>;
pub type BoxStreamNonSend<T, E> = Box<Stream<Item = T, Error = E>>;

pub trait FutureExt: Future + Sized {
    /// Map a `Future` to have `Item=()` and `Error=()`. This is
    /// useful when a future is being used to drive a computation
    /// but the actual results aren't interesting (such as when used
    /// with `Handle::spawn()`).
    fn discard(self) -> Discard<Self> {
        Discard(self)
    }

    /// Create a `Send`able boxed version of this `Future`.
    #[inline]
    fn boxify(self) -> BoxFuture<Self::Item, Self::Error>
    where
        Self: 'static + Send,
    {
        // TODO: (sid0) T21801845 rename to 'boxed' once gone from upstream.
        Box::new(self)
    }

    /// Create a non-`Send`able boxed version of this `Future`.
    #[inline]
    fn boxify_nonsend(self) -> BoxFutureNonSend<Self::Item, Self::Error>
    where
        Self: 'static,
    {
        Box::new(self)
    }
}

impl<T> FutureExt for T
where
    T: Future,
{
}

pub trait StreamExt: Stream {
    /// Fork elements in a stream out to two sinks, depending on a predicate
    ///
    /// If the predicate returns false, send the item to `out1`, otherwise to
    /// `out2`. `streamfork()` acts in a similar manner to `forward()` in that it
    /// keeps operating until the input stream ends, and then returns everything
    /// in the resulting Future.
    ///
    /// The predicate returns a `Result` so that it can fail (if there's a malformed
    /// input that can't be assigned to either output).
    fn streamfork<Out1, Out2, F, E>(
        self,
        out1: Out1,
        out2: Out2,
        pred: F,
    ) -> streamfork::Forker<Self, Out1, Out2, F>
    where
        Self: Sized,
        Out1: Sink<SinkItem = Self::Item>,
        Out2: Sink<SinkItem = Self::Item, SinkError = Out1::SinkError>,
        F: FnMut(&Self::Item) -> Result<bool, E>,
        E: From<Self::Error> + From<Out1::SinkError> + From<Out2::SinkError>,
    {
        streamfork::streamfork(self, out1, out2, pred)
    }

    fn take_while_wrapper<P, R>(self, pred: P) -> TakeWhile<Self, P, R>
    where
        P: FnMut(&Self::Item) -> R,
        R: IntoFuture<Item = bool, Error = Self::Error>,
        Self: Sized,
    {
        stream_wrappers::take_while::new(self, pred)
    }

    fn encode<Enc>(self, encoder: Enc) -> encode::LayeredEncoder<Self, Enc>
    where
        Self: Sized,
        Enc: Encoder<Item = Self::Item>,
    {
        encode::encode(self, encoder)
    }

    fn enumerate(self) -> Enumerate<Self>
    where
        Self: Sized,
    {
        Enumerate::new(self)
    }

    /// Create a `Send`able boxed version of this `Stream`.
    #[inline]
    fn boxify(self) -> BoxStream<Self::Item, Self::Error>
    where
        Self: 'static + Send + Sized,
    {
        // TODO: (sid0) T21801845 rename to 'boxed' once gone from upstream.
        Box::new(self)
    }

    /// Create a non-`Send`able boxed version of this `Stream`.
    #[inline]
    fn boxify_nonsend(self) -> BoxStreamNonSend<Self::Item, Self::Error>
    where
        Self: 'static + Sized,
    {
        Box::new(self)
    }
}

impl<T> StreamExt for T
where
    T: Stream,
{
}

pub trait AsyncReadExt: AsyncRead + Sized {
    fn framed_stream<C>(self, codec: C) -> FramedStream<Self, C>
    where
        C: Decoder,
    {
        frame::framed_stream(self, codec)
    }
}

impl<T> AsyncReadExt for T
where
    T: AsyncRead,
{
}

pub trait StreamLayeredExt: Stream<Item = Bytes> {
    fn decode<Dec>(self, decoder: Dec) -> decode::LayeredDecode<Self, Dec>
    where
        Self: Sized,
        Dec: Decoder;
}

impl<T> StreamLayeredExt for T
where
    T: Stream<Item = Bytes>,
{
    fn decode<Dec>(self, decoder: Dec) -> decode::LayeredDecode<Self, Dec>
    where
        Self: Sized,
        Dec: Decoder,
    {
        decode::decode(self, decoder)
    }
}

pub struct Enumerate<In> {
    inner: In,
    count: usize,
}

impl<In> Enumerate<In> {
    fn new(inner: In) -> Self {
        Enumerate {
            inner: inner,
            count: 0,
        }
    }
}

impl<In: Stream> Stream for Enumerate<In> {
    type Item = (usize, In::Item);
    type Error = In::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Err(err) => Err(err),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(v))) => {
                let c = self.count;
                self.count += 1;
                Ok(Async::Ready(Some((c, v))))
            }
        }
    }
}

/// A convenience macro for working with `io::Result<T>` from the `Read` and
/// `Write` traits.
///
/// This macro takes `io::Result<T>` as input, and returns `Poll<T, io::Error>`
/// as the output. If the input type is of the `Err` variant, then
/// `Poll::NotReady` is returned if it indicates `WouldBlock` or otherwise `Err`
/// is returned.
#[macro_export]
macro_rules! handle_nb {
    ($e:expr) => (match $e {
        Ok(t) => Ok(::futures::Async::Ready(t)),
        Err(ref e) if e.kind() == ::std::io::ErrorKind::WouldBlock => {
            Ok(::futures::Async::NotReady)
        }
        Err(e) => Err(e),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Stream;
    use futures::stream;
    use futures::sync::mpsc;

    #[derive(Debug)]
    struct MyErr;

    impl<T> From<mpsc::SendError<T>> for MyErr {
        fn from(_: mpsc::SendError<T>) -> Self {
            MyErr
        }
    }


    #[test]
    fn discard() {
        use futures::sync::mpsc;
        use tokio_core::reactor::Core;

        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let (tx, rx) = mpsc::channel(1);

        let xfer = stream::iter_ok::<_, MyErr>(vec![123]).forward(tx);

        handle.spawn(xfer.discard());

        match core.run(rx.collect()) {
            Ok(v) => assert_eq!(v, vec![123]),
            bad => panic!("bad {:?}", bad),
        }
    }

    #[test]
    fn enumerate() {
        let s = stream::iter_ok::<_, ()>(vec!["hello", "there", "world"]);
        let es = Enumerate::new(s);
        let v = es.collect().wait();

        assert_eq!(v, Ok(vec![(0, "hello"), (1, "there"), (2, "world")]));
    }
}
