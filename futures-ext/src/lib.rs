// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

use bytes::Bytes;
use futures::sync::{mpsc, oneshot};
use futures::{
    future, stream, try_ready, Async, AsyncSink, Future, IntoFuture, Poll, Sink, Stream,
};
use std::{
    fmt::Debug,
    io as std_io,
    time::{Duration, Instant},
};
use tokio::timer::Delay;
use tokio_io::{
    codec::{Decoder, Encoder},
    AsyncWrite,
};
use tokio_threadpool::blocking;

pub mod bounded_traversal;
mod bytes_stream;
pub mod decode;
pub mod encode;
mod futures_ordered;
pub mod io;
mod launch;
mod select_all;
mod split_err;
mod stream_clone;
mod stream_wrappers;
mod streamfork;

pub use crate::bytes_stream::{BytesStream, BytesStreamFuture};
pub use crate::futures_ordered::{futures_ordered, FuturesOrdered};
pub use crate::launch::top_level_launch;
pub use crate::select_all::{select_all, SelectAll};
pub use crate::split_err::split_err;
pub use crate::stream_clone::stream_clone;
pub use crate::stream_wrappers::{
    BoxStreamWrapper, CollectNoConsume, CollectTo, StreamWrapper, TakeWhile,
};

/// Map `Item` and `Error` to `()`
///
/// Adapt an existing `Future` to return unit `Item` and `Error`, while still
/// waiting for the underlying `Future` to complete.
#[must_use = "futures do nothing unless you `.await` or poll them"]
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

/// Send an item over an mpsc channel, discarding both the sender and receiver-closed errors. This
/// should be used when the receiver being closed makes sending values moot, since no one is
/// interested in the results any more.
///
/// `E` is an arbitrary error type useful for getting types to match up, but it will never be
/// produced by the returned future.
#[inline]
pub fn send_discard<T, E>(
    sender: mpsc::Sender<T>,
    value: T,
) -> impl Future<Item = (), Error = E> + Send
where
    T: Send,
    E: Send,
{
    sender.send(value).then(|_| Ok(()))
}

// Replacements for BoxFuture and BoxStream, deprecated in upstream futures-rs.
pub type BoxFuture<T, E> = Box<dyn Future<Item = T, Error = E> + Send>;
pub type BoxFutureNonSend<T, E> = Box<dyn Future<Item = T, Error = E>>;
pub type BoxStream<T, E> = Box<dyn Stream<Item = T, Error = E> + Send>;
pub type BoxStreamNonSend<T, E> = Box<dyn Stream<Item = T, Error = E>>;

/// Do something with an error if the future failed.
///
/// This is created by the `FutureExt::inspect_err` method.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct InspectErr<A, F>
where
    A: Future,
{
    future: A,
    f: Option<F>,
}

impl<A, F> Future for InspectErr<A, F>
where
    A: Future,
    F: FnOnce(&A::Error),
{
    type Item = A::Item;
    type Error = A::Error;

    fn poll(&mut self) -> Poll<A::Item, A::Error> {
        match self.future.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(e)) => Ok(Async::Ready(e)),
            Err(e) => {
                self.f.take().map_or_else(
                    // Act like a fused future
                    || Ok(Async::NotReady),
                    |func| {
                        func(&e);
                        Err(e)
                    },
                )
            }
        }
    }
}

/// Inspect the Result returned by a future
///
/// This is created by the `FutureExt::inspect_result` method.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct InspectResult<A, F>
where
    A: Future,
{
    future: A,
    f: Option<F>,
}

impl<A, F> Future for InspectResult<A, F>
where
    A: Future,
    F: FnOnce(Result<&A::Item, &A::Error>),
{
    type Item = A::Item;
    type Error = A::Error;

    fn poll(&mut self) -> Poll<A::Item, A::Error> {
        match self.future.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(i)) => self.f.take().map_or_else(
                // Act like a fused future
                || Ok(Async::NotReady),
                |func| {
                    func(Ok(&i));
                    Ok(Async::Ready(i))
                },
            ),

            Err(e) => self.f.take().map_or_else(
                // Act like a fused future
                || Ok(Async::NotReady),
                |func| {
                    func(Err(&e));
                    Err(e)
                },
            ),
        }
    }
}

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
        // TODO: (rain1) T21801845 rename to 'boxed' once gone from upstream.
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

    fn left_future<B>(self) -> future::Either<Self, B> {
        future::Either::A(self)
    }

    fn right_future<A>(self) -> future::Either<A, Self> {
        future::Either::B(self)
    }

    fn inspect_err<F>(self, f: F) -> InspectErr<Self, F>
    where
        F: FnOnce(&Self::Error) -> (),
        Self: Sized,
    {
        InspectErr {
            future: self,
            f: Some(f),
        }
    }

    fn inspect_result<F>(self, f: F) -> InspectResult<Self, F>
    where
        F: FnOnce(Result<&Self::Item, &Self::Error>) -> (),
        Self: Sized,
    {
        InspectResult {
            future: self,
            f: Some(f),
        }
    }
}

impl<T> FutureExt for T where T: Future {}

pub struct BufferedParams {
    pub weight_limit: u64,
    pub buffer_size: usize,
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
    ) -> streamfork::Forker<Self, Out1, Out2, F, E>
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

    fn collect_no_consume(self) -> CollectNoConsume<Self>
    where
        Self: Sized,
    {
        stream_wrappers::collect_no_consume::new(self)
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

    /// Creates a stream wrapper and a future. The future will resolve into the wrapped stream when
    /// the stream wrapper returns None. It uses ConservativeReceiver to ensure that deadlocks are
    /// easily caught when one tries to poll on the receiver before consuming the stream.
    fn return_remainder(self) -> (ReturnRemainder<Self>, ConservativeReceiver<Self>)
    where
        Self: Sized,
    {
        ReturnRemainder::new(self)
    }

    /// Whether this stream is empty.
    ///
    /// This will consume one element from the stream if returned.
    fn is_empty<'a>(self) -> Box<dyn Future<Item = bool, Error = Self::Error> + Send + 'a>
    where
        Self: 'a + Send + Sized,
    {
        Box::new(
            self.into_future()
                .map(|(first, _rest)| first.is_none())
                .map_err(|(err, _rest)| err),
        )
    }

    /// Whether this stream is not empty (has at least one element).
    ///
    /// This will consume one element from the stream if returned.
    fn not_empty<'a>(self) -> Box<dyn Future<Item = bool, Error = Self::Error> + Send + 'a>
    where
        Self: 'a + Send + Sized,
    {
        Box::new(
            self.into_future()
                .map(|(first, _rest)| first.is_some())
                .map_err(|(err, _rest)| err),
        )
    }

    /// Create a `Send`able boxed version of this `Stream`.
    #[inline]
    fn boxify(self) -> BoxStream<Self::Item, Self::Error>
    where
        Self: 'static + Send + Sized,
    {
        // TODO: (rain1) T21801845 rename to 'boxed' once gone from upstream.
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

    fn left_stream<B>(self) -> StreamEither<Self, B>
    where
        Self: Sized,
    {
        StreamEither::A(self)
    }

    fn right_stream<A>(self) -> StreamEither<A, Self>
    where
        Self: Sized,
    {
        StreamEither::B(self)
    }

    // It's different from tokio::timer::Timeout in that it sets a timeout on the whole Stream,
    // not just on a single Stream item
    fn whole_stream_timeout(self, duration: Duration) -> StreamWithTimeout<Self>
    where
        Self: Sized,
    {
        StreamWithTimeout {
            stream: self,
            delay: Delay::new(Instant::now() + duration),
        }
    }

    fn batch(self, limit: usize) -> BatchStream<Self>
    where
        Self: Sized,
    {
        BatchStream::new(self, limit)
    }

    // Like `buffered()` call, but can also limit number of futures in a buffer by "weight".
    fn buffered_weight_limited<I, E, Fut>(
        self,
        params: BufferedParams,
    ) -> WeightLimitedBufferedStream<Self, I, E>
    where
        Self: Sized + Send + 'static,
        Self: Stream<Item = (Fut, u64), Error = E>,
        Fut: Future<Item = I, Error = E>,
    {
        WeightLimitedBufferedStream::new(params, self)
    }

    fn collect_to<C: Default + Extend<Self::Item>>(self) -> CollectTo<Self, C>
    where
        Self: Sized,
    {
        CollectTo::new(self)
    }
}

impl<T> StreamExt for T where T: Stream {}

pub struct WeightLimitedBufferedStream<S, I, E> {
    queue: stream::FuturesOrdered<BoxFuture<(I, u64), E>>,
    current_weight: u64,
    weight_limit: u64,
    max_buffer_size: usize,
    stream: stream::Fuse<S>,
}

impl<S, I, E> WeightLimitedBufferedStream<S, I, E>
where
    S: Stream,
{
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

impl<S, Fut, I: 'static, E: 'static> Stream for WeightLimitedBufferedStream<S, I, E>
where
    S: Stream<Item = (Fut, u64), Error = E>,
    Fut: Future<Item = I, Error = E> + Send + 'static,
{
    type Item = I;
    type Error = E;

    fn poll(&mut self) -> Poll<Option<Self::Item>, E> {
        // First up, try to spawn off as many futures as possible by filling up
        // our slab of futures.
        while self.queue.len() < self.max_buffer_size && self.current_weight < self.weight_limit {
            let future = match self.stream.poll()? {
                Async::Ready(Some((s, weight))) => {
                    self.current_weight += weight;
                    s.map(move |val| (val, weight)).boxify()
                }
                Async::Ready(None) | Async::NotReady => break,
            };

            self.queue.push(future);
        }

        // Try polling a new future
        if let Some((val, weight)) = try_ready!(self.queue.poll()) {
            self.current_weight -= weight;
            return Ok(Async::Ready(Some(val)));
        }

        // If we've gotten this far, then there are no events for us to process
        // and nothing was ready, so figure out if we're not done yet  or if
        // we've reached the end.
        if self.stream.is_done() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
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
        Enumerate { inner, count: 0 }
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

pub enum StreamEither<A, B> {
    A(A),
    B(B),
}

impl<A, B> Stream for StreamEither<A, B>
where
    A: Stream,
    B: Stream<Item = A::Item, Error = A::Error>,
{
    type Item = A::Item;
    type Error = A::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self {
            StreamEither::A(a) => a.poll(),
            StreamEither::B(b) => b.poll(),
        }
    }
}

/// This is a wrapper around oneshot::Receiver that will return error when the receiver was polled
/// and the result was not ready. This is a very strict way of preventing deadlocks in code when
/// receiver is polled before the sender has send the result
pub struct ConservativeReceiver<T>(oneshot::Receiver<T>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConservativeReceiverError {
    Canceled,
    ReceiveBeforeSend,
}

impl ::std::error::Error for ConservativeReceiverError {
    fn description(&self) -> &str {
        match self {
            &ConservativeReceiverError::Canceled => "oneshot canceled",
            &ConservativeReceiverError::ReceiveBeforeSend => "recv called on channel before send",
        }
    }
}

impl ::std::fmt::Display for ConservativeReceiverError {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            &ConservativeReceiverError::Canceled => write!(fmt, "oneshot canceled"),
            &ConservativeReceiverError::ReceiveBeforeSend => {
                write!(fmt, "recv called on channel before send")
            }
        }
    }
}

impl ::std::convert::From<oneshot::Canceled> for ConservativeReceiverError {
    fn from(_: oneshot::Canceled) -> ConservativeReceiverError {
        ConservativeReceiverError::Canceled
    }
}

impl<T> ConservativeReceiver<T> {
    pub fn new(recv: oneshot::Receiver<T>) -> Self {
        ConservativeReceiver(recv)
    }
}

impl<T> Future for ConservativeReceiver<T> {
    type Item = T;
    type Error = ConservativeReceiverError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.0.poll()? {
            Async::Ready(item) => Ok(Async::Ready(item)),
            Async::NotReady => Err(ConservativeReceiverError::ReceiveBeforeSend),
        }
    }
}

pub struct ReturnRemainder<In> {
    inner: Option<In>,
    send: Option<oneshot::Sender<In>>,
}

impl<In> ReturnRemainder<In> {
    fn new(inner: In) -> (Self, ConservativeReceiver<In>) {
        let (send, recv) = oneshot::channel();
        (
            Self {
                inner: Some(inner),
                send: Some(send),
            },
            ConservativeReceiver::new(recv),
        )
    }
}

impl<In: Stream> Stream for ReturnRemainder<In> {
    type Item = In::Item;
    type Error = In::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let maybe_item = match self.inner {
            Some(ref mut inner) => try_ready!(inner.poll()),
            None => return Ok(Async::Ready(None)),
        };

        if maybe_item.is_none() {
            let inner = self
                .inner
                .take()
                .expect("inner was just polled, should be some");
            let send = self.send.take().expect("send is None iff inner is None");
            // The Receiver will handle errors
            let _ = send.send(inner);
        }

        Ok(Async::Ready(maybe_item))
    }
}

pub enum StreamTimeoutError {
    Error(failure::Error),
    Timeout,
}

pub struct StreamWithTimeout<S> {
    delay: Delay,
    stream: S,
}

impl<S: Stream<Error = failure::Error>> Stream for StreamWithTimeout<S> {
    type Item = S::Item;
    type Error = StreamTimeoutError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.delay.poll() {
            Ok(Async::Ready(())) => {
                return Err(StreamTimeoutError::Timeout);
            }
            Err(err) => {
                return Err(StreamTimeoutError::Error(failure::err_msg(format!(
                    "internal error: timeout failed {}",
                    err
                ))));
            }
            _ => {}
        };

        match self.stream.poll() {
            Ok(Async::Ready(item)) => Ok(Async::Ready(item)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => Err(StreamTimeoutError::Error(err)),
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
#[rustfmt::skip]
macro_rules! handle_nb {
    ($e:expr) => {
        match $e {
            Ok(t) => Ok(::futures::Async::Ready(t)),
            Err(ref e) if e.kind() == ::std::io::ErrorKind::WouldBlock => {
                Ok(::futures::Async::NotReady)
            }
            Err(e) => Err(e),
        }
    };
}

/// Macro that can be used like `?` operator, but in the context where the expected return type is
/// BoxFuture. The result of it is either Ok part of Result or immediate returning the Err part
/// converted into BoxFuture.
#[macro_export]
#[rustfmt::skip]
macro_rules! try_boxfuture {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => return $crate::FutureExt::boxify(::futures::future::err(e.into())),
        }
    };
}

/// Macro that can be used like `?` operator, but in the context where the expected return type is
/// BoxStream. The result of it is either Ok part of Result or immediate returning the Err part
/// converted into BoxStream.
#[macro_export]
#[rustfmt::skip]
macro_rules! try_boxstream {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => return $crate::StreamExt::boxify(::futures::stream::once(Err(e.into()))),
        }
    };
}

/// Macro that can be used like ensure! macro from failure crate, but in the context where the
/// expected return type is BoxFuture. Exits a function early with an Error if the condition is not
/// satisfied.
#[macro_export]
#[rustfmt::skip]
macro_rules! ensure_boxfuture {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            return $crate::FutureExt::boxify(::futures::future::err($e.into()));
        }
    };
}

/// Macro that can be used like ensure! macro from failure crate, but in the context where the
/// expected return type is BoxStream. Exits a function early with an Error if the condition is not
/// satisfied.
#[macro_export]
#[rustfmt::skip]
macro_rules! ensure_boxstream {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            return $crate::StreamExt::boxify(::futures::stream::once(Err($e.into())));
        }
    };
}

/// Macro that can be used like `?` operator, but in the context where the expected return type is
///  a left future. The result of it is either Ok part of Result or immediate returning the Err
//part / converted into a  a left future.
#[macro_export]
#[rustfmt::skip]
macro_rules! try_left_future {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => return ::futures::future::err(e.into()).left_future(),
        }
    };
}

/// Take a future, and run it on its own task, returning the result to the caller. This permits
/// Rust to run the spawned future on a different thread to the task that spawned it, thus adding
/// parallelism if used sensibly.
/// Note that the spawning here is lazy - the new task will not be spawned if the returned future
/// is dropped before it's polled.
pub fn spawn_future<T, E, Fut, IntoFut>(f: IntoFut) -> impl Future<Item = T, Error = E>
where
    IntoFut: IntoFuture<Item = T, Error = E, Future = Fut>,
    Fut: Future<Item = T, Error = E> + Send + 'static,
    T: Send + 'static,
    E: From<futures::Canceled> + Send + 'static,
{
    let (tx, rx) = oneshot::channel();

    let fut = f.into_future().then(|res| {
        let _ = tx.send(res);
        Ok(())
    });

    future::lazy(move || {
        let _ = tokio::spawn(fut);
        rx.from_err().and_then(|v| v)
    })
}

/// Given an `FnMut` closure, create a `Future` that will eventually execute the closure using
/// Tokio's `blocking` mechanism, so that it is safe to call blocking code inside the closure
/// without preventing other tasks from making progress.
/// This returns a lazy future - it will not even attempt to run the blocking code until you poll
/// the future.
/// Note that this does not spawn the future onto its own task - use `asynchronize` below if you
/// need to run the blocking code on its own thread, rather than letting it block this task.
pub fn closure_to_blocking_future<T, E, Func>(f: Func) -> impl Future<Item = T, Error = E>
where
    Func: FnMut() -> Result<T, E>,
    E: From<tokio_threadpool::BlockingError>,
{
    let mut func = f;
    future::lazy(|| future::poll_fn(move || blocking(&mut func)))
        .map_err(E::from)
        .and_then(|res| res) // flatten Ok(res) => res
}

///  This method allows us to take synchronous code, schedule it on the default tokio thread pool
/// and convert it to the future. It's the combination of `spawn_future` (which runs a future on
/// another thread) and `closure_to_blocking_future` (which turns a closure into a future).
pub fn asynchronize<Func, T, E>(f: Func) -> impl Future<Item = T, Error = E>
where
    Func: FnMut() -> Result<T, E> + Send + 'static,
    T: Send + 'static,
    E: From<tokio_threadpool::BlockingError> + From<futures::Canceled> + Send + 'static,
{
    let fut = closure_to_blocking_future(f);

    spawn_future(fut)
}

/// Simple adapter from `Sink` interface to `AsyncWrite` interface.
/// It can be useful to convert from the interface that supports only AsyncWrite, and get
/// Stream as a result. See pseudocode below
///
///  ```
///     fn async_write_interface(writer: &mut AsyncWrite) -> impl Future<(), Error> {
///       ...
///     }
///
///     use futures::sync::mpsc;
///     let (sender, receiver) = mpsc::channel(1);
///
///     tokio::spawn(
///        async_write_interface(SinkToAsyncWrite::new(sender))
///            .map_err(|err| {})
///     );
///
///     // receiver is a stream of values written from async_write_interface
///  ```
pub struct SinkToAsyncWrite<S> {
    sink: S,
}

impl<S> SinkToAsyncWrite<S> {
    pub fn new(sink: S) -> Self {
        SinkToAsyncWrite { sink }
    }
}

fn create_std_error<E: Debug>(err: E) -> std_io::Error {
    std_io::Error::new(std_io::ErrorKind::Other, format!("{:?}", err))
}

impl<E, S> std_io::Write for SinkToAsyncWrite<S>
where
    S: Sink<SinkItem = Bytes, SinkError = E>,
    E: Debug,
{
    fn write(&mut self, buf: &[u8]) -> ::std::io::Result<usize> {
        let bytes = Bytes::from(buf);
        match self.sink.start_send(bytes) {
            Ok(AsyncSink::Ready) => Ok(buf.len()),
            Ok(AsyncSink::NotReady(_)) => Err(std_io::Error::new(
                std_io::ErrorKind::WouldBlock,
                "channel is busy",
            )),
            Err(err) => Err(create_std_error(err)),
        }
    }

    fn flush(&mut self) -> std_io::Result<()> {
        match self.sink.poll_complete() {
            Ok(Async::Ready(())) => Ok(()),
            Ok(Async::NotReady) => Err(std_io::Error::new(
                std_io::ErrorKind::WouldBlock,
                "channel is busy",
            )),
            Err(err) => Err(create_std_error(err)),
        }
    }
}

impl<E, S> AsyncWrite for SinkToAsyncWrite<S>
where
    S: Sink<SinkItem = Bytes, SinkError = E>,
    E: Debug,
{
    fn shutdown(&mut self) -> Poll<(), std_io::Error> {
        match self.sink.close() {
            Ok(res) => Ok(res),
            Err(err) => Err(create_std_error(err)),
        }
    }
}

/// It's a combinator that converts Stream<A> into Stream<Vec<A>>.
/// So interface is similar to `.chunks()` method, but there's an important difference:
/// BatchStream won't wait until the whole batch fills up i.e. as soon as underlying stream
/// return NotReady, then new batch is returned from BatchStream
pub struct BatchStream<S>
where
    S: Stream,
{
    inner: stream::Fuse<S>,
    err: Option<S::Error>,
    limit: usize,
}

impl<S: Stream> BatchStream<S> {
    pub fn new(s: S, limit: usize) -> Self {
        Self {
            inner: s.fuse(),
            err: None,
            limit,
        }
    }
}

impl<S: Stream> Stream for BatchStream<S> {
    type Item = Vec<S::Item>;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut batch = vec![];

        if let Some(err) = self.err.take() {
            return Err(err);
        }

        while batch.len() < self.limit {
            match self.inner.poll() {
                Ok(Async::Ready(Some(v))) => batch.push(v),
                Ok(Async::NotReady) | Ok(Async::Ready(None)) => break,
                Err(err) => {
                    self.err = Some(err);
                    break;
                }
            }
        }

        if batch.is_empty() {
            if let Some(err) = self.err.take() {
                return Err(err);
            }

            if self.inner.is_done() {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
            }
        } else {
            Ok(Async::Ready(Some(batch)))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::time::{self, Duration};

    use assert_matches::assert_matches;
    use futures::stream;
    use futures::sync::mpsc;
    use futures::Stream;

    use cloned::cloned;
    use futures::future::{err, ok};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    #[derive(Debug)]
    struct MyErr;

    impl<T> From<mpsc::SendError<T>> for MyErr {
        fn from(_: mpsc::SendError<T>) -> Self {
            MyErr
        }
    }

    #[test]
    #[ignore]
    // In some cases it fails with a longer duration than specified
    fn asynchronize_parallel() {
        const SLEEP_TIME: Duration = time::Duration::from_millis(20);
        const THREAD_COUNT: usize = 20;

        assert!(
            THREAD_COUNT > 10,
            "Thread count too small to prove parallelism"
        );

        let mut runtime = tokio::runtime::Builder::new()
            .name_prefix("my-runtime-worker-")
            .core_threads(THREAD_COUNT)
            .build()
            .unwrap();
        fn sleep() -> Result<(), failure::Error> {
            std::thread::sleep(SLEEP_TIME);
            Ok(())
        }

        let futures: Vec<_> = std::iter::repeat_with(|| asynchronize(|| sleep()))
        // This count needs to be much greater than 2, so that if we serialize operations, we see
        // an issue
            .take(THREAD_COUNT)
            .collect();
        let start = time::Instant::now();
        let _ = runtime.block_on(future::join_all(futures));
        let run_time = start.elapsed();
        assert!(
            run_time < SLEEP_TIME * 2,
            "Parallel sleep time {:#?} much greater than {:#?} for {:#?} threads - each thread sleeps for {:#?}",
            run_time,
            SLEEP_TIME * 2,
            THREAD_COUNT,
            SLEEP_TIME
        );
    }

    #[test]
    fn discard() {
        use futures::sync::mpsc;

        let mut runtime = Runtime::new().unwrap();

        let (tx, rx) = mpsc::channel(1);

        let xfer = stream::iter_ok::<_, MyErr>(vec![123]).forward(tx);

        runtime.spawn(xfer.discard());

        match runtime.block_on(rx.collect()) {
            Ok(v) => assert_eq!(v, vec![123]),
            bad => panic!("bad {:?}", bad),
        }
    }

    #[test]
    fn inspect_err() {
        let count = Arc::new(AtomicUsize::new(0));
        cloned!(count as count_cloned);
        let mut runtime = Runtime::new().unwrap();
        let work = err::<i32, i32>(42).inspect_err(move |e| {
            assert_eq!(42, *e);
            count_cloned.fetch_add(1, Ordering::SeqCst);
        });
        if let Ok(_) = runtime.block_on(work) {
            panic!("future is supposed to fail");
        }
        assert_eq!(1, count.load(Ordering::SeqCst));
    }

    #[test]
    fn inspect_ok() {
        let count = Arc::new(AtomicUsize::new(0));
        cloned!(count as count_cloned);
        let mut runtime = Runtime::new().unwrap();
        let work = ok::<i32, i32>(42).inspect_err(move |_| {
            count_cloned.fetch_add(1, Ordering::SeqCst);
        });
        if let Err(_) = runtime.block_on(work) {
            panic!("future is supposed to succeed");
        }
        assert_eq!(0, count.load(Ordering::SeqCst));
    }

    #[test]
    fn inspect_result() {
        let count = Arc::new(AtomicUsize::new(0));
        cloned!(count as count_cloned);
        let mut runtime = Runtime::new().unwrap();
        let work = err::<i32, i32>(42).inspect_result(move |res| {
            if let Err(e) = res {
                assert_eq!(42, *e);
                count_cloned.fetch_add(1, Ordering::SeqCst);
            } else {
                count_cloned.fetch_add(2, Ordering::SeqCst);
            }
        });
        if let Ok(_) = runtime.block_on(work) {
            panic!("future is supposed to fail");
        }
        assert_eq!(1, count.load(Ordering::SeqCst));
    }

    #[test]
    fn enumerate() {
        let s = stream::iter_ok::<_, ()>(vec!["hello", "there", "world"]);
        let es = Enumerate::new(s);
        let v = es.collect().wait();

        assert_eq!(v, Ok(vec![(0, "hello"), (1, "there"), (2, "world")]));
    }

    #[test]
    fn empty() {
        let mut s = stream::empty::<(), ()>();
        // Ensure that the stream doesn't have to be consumed.
        assert!(s.by_ref().is_empty().wait().unwrap());
        assert!(!s.not_empty().wait().unwrap());

        let mut s = stream::once::<_, ()>(Ok("foo"));
        assert!(!s.by_ref().is_empty().wait().unwrap());
        // The above is_empty would consume the first element, so the stream has to be
        // reinitialized.
        let s = stream::once::<_, ()>(Ok("foo"));
        assert!(s.not_empty().wait().unwrap());
    }

    #[test]
    fn return_remainder() {
        use futures::future::poll_fn;

        let s = stream::iter_ok::<_, ()>(vec!["hello", "there", "world"]).fuse();
        let (mut s, mut remainder) = s.return_remainder();

        let mut runtime = Runtime::new().unwrap();
        let res: Result<(), ()> = runtime.block_on(poll_fn(move || {
            assert_matches!(
                remainder.poll(),
                Err(ConservativeReceiverError::ReceiveBeforeSend)
            );

            assert_eq!(s.poll(), Ok(Async::Ready(Some("hello"))));
            assert_matches!(
                remainder.poll(),
                Err(ConservativeReceiverError::ReceiveBeforeSend)
            );

            assert_eq!(s.poll(), Ok(Async::Ready(Some("there"))));
            assert_matches!(
                remainder.poll(),
                Err(ConservativeReceiverError::ReceiveBeforeSend)
            );

            assert_eq!(s.poll(), Ok(Async::Ready(Some("world"))));
            assert_matches!(
                remainder.poll(),
                Err(ConservativeReceiverError::ReceiveBeforeSend)
            );

            assert_eq!(s.poll(), Ok(Async::Ready(None)));
            match remainder.poll() {
                Ok(Async::Ready(s)) => assert!(s.is_done()),
                bad => panic!("unexpected result: {:?}", bad),
            }

            Ok(Async::Ready(()))
        }));

        assert_matches!(res, Ok(()));
    }

    fn assert_flush<E, S>(sink: &mut SinkToAsyncWrite<S>)
    where
        S: Sink<SinkItem = Bytes, SinkError = E>,
        E: Debug,
    {
        use std::io::Write;
        loop {
            let flush_res = sink.flush();
            if let Ok(_) = flush_res {
                break;
            }
            if let Err(ref e) = flush_res {
                println!("after flush error");
                assert_eq!(e.kind(), std_io::ErrorKind::WouldBlock);
            }
        }
    }

    fn assert_shutdown<E, S>(sink: &mut SinkToAsyncWrite<S>)
    where
        S: Sink<SinkItem = Bytes, SinkError = E>,
        E: Debug,
    {
        loop {
            let shutdown_res = sink.shutdown();
            if let Ok(_) = shutdown_res {
                break;
            }
            if let Err(ref e) = shutdown_res {
                println!("after flush error");
                assert_eq!(e.kind(), std_io::ErrorKind::WouldBlock);
            }
        }
    }

    #[test]
    fn sink_to_async_write() {
        use futures::sync::mpsc;
        use std::io::Write;

        async_unit::tokio_unit_test(|| {
            let (tx, rx) = mpsc::channel::<Bytes>(1);

            let messages_num = 10;
            tokio::spawn(Ok(()).into_future().map(move |()| {
                let mut async_write = SinkToAsyncWrite::new(tx);
                for i in 0..messages_num {
                    loop {
                        let res = async_write.write(format!("{}", i).as_bytes());
                        if let Err(ref e) = res {
                            assert_eq!(e.kind(), std_io::ErrorKind::WouldBlock);
                            assert_flush(&mut async_write);
                        } else {
                            break;
                        }
                    }
                }

                assert_flush(&mut async_write);
                assert_shutdown(&mut async_write);
            }));

            let res = rx.collect().wait().unwrap();
            assert_eq!(res.len(), messages_num);
        })
    }

    #[test]
    fn whole_stream_timeout_test() {
        use futures::Stream;
        use tokio::timer::Interval;

        let count = Arc::new(AtomicUsize::new(0));
        let mut runtime = Runtime::new().unwrap();
        let f = Interval::new(Instant::now(), Duration::new(1, 0))
            .map({
                let count = count.clone();
                move |item| {
                    count.fetch_add(1, Ordering::Relaxed);
                    item
                }
            })
            .map_err(|_| failure::err_msg("error"))
            .take(10)
            .whole_stream_timeout(Duration::new(3, 0))
            .collect();

        let res = runtime.block_on(f);
        assert!(res.is_err());
        match res {
            Err(StreamTimeoutError::Timeout) => {}
            _ => {
                panic!("expected timeout");
            }
        };

        assert!(count.load(Ordering::Relaxed) < 5);
    }

    #[test]
    fn test_buffered() {
        fn create_stream() -> (Arc<AtomicUsize>, BoxStream<(BoxFuture<(), ()>, u64), ()>) {
            let s: BoxStream<(BoxFuture<(), ()>, u64), ()> = stream::iter_ok(vec![
                (future::ok(()).boxify(), 100),
                (future::ok(()).boxify(), 2),
            ])
            .boxify();

            let counter = Arc::new(AtomicUsize::new(0));

            (
                counter.clone(),
                s.inspect({
                    move |_val| {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .boxify(),
            )
        }

        let mut runtime = tokio::runtime::Builder::new().build().unwrap();

        let (counter, s) = create_stream();
        let params = BufferedParams {
            weight_limit: 10,
            buffer_size: 10,
        };
        let s = s.buffered_weight_limited(params);
        if let Ok((Some(()), s)) = runtime.block_on(s.into_future()) {
            assert_eq!(counter.load(Ordering::SeqCst), 1);
            assert_eq!(runtime.block_on(s.collect()).unwrap().len(), 1);
            assert_eq!(counter.load(Ordering::SeqCst), 2);
        } else {
            panic!("failed to block on a stream");
        }

        let (counter, s) = create_stream();
        let params = BufferedParams {
            weight_limit: 200,
            buffer_size: 10,
        };
        let s = s.buffered_weight_limited(params);
        if let Ok((Some(()), s)) = runtime.block_on(s.into_future()) {
            assert_eq!(counter.load(Ordering::SeqCst), 2);
            assert_eq!(runtime.block_on(s.collect()).unwrap().len(), 1);
            assert_eq!(counter.load(Ordering::SeqCst), 2);
        } else {
            panic!("failed to block on a stream");
        }
    }

    use std::collections::HashSet;
    use std::iter::Iterator;

    fn assert_same_elements<I, T>(src: Vec<I>, iter: T)
    where
        I: Copy + Debug + Ord,
        T: IntoIterator<Item = I>,
    {
        let mut dst_sorted: Vec<I> = iter.into_iter().collect();
        dst_sorted.sort();

        let mut src_sorted = src;
        src_sorted.sort();

        assert_eq!(src_sorted, dst_sorted);
    }

    #[test]
    fn collect_into_vec() {
        let items = vec![1, 2, 3];
        let future = futures::stream::iter_ok::<_, ()>(items.clone()).collect_to();
        let mut runtime = Runtime::new().unwrap();
        match runtime.block_on::<_, Vec<i32>, _>(future) {
            Ok(collections) => assert_same_elements(items, collections),
            Err(_) => panic!("future is supposed to succeed"),
        }
    }

    #[test]
    fn collect_into_set() {
        let items = vec![1, 2, 3];
        let future = futures::stream::iter_ok::<_, ()>(items.clone()).collect_to();
        let mut runtime = Runtime::new().unwrap();
        match runtime.block_on::<_, HashSet<i32>, _>(future) {
            Ok(collections) => assert_same_elements(items, collections),
            Err(_) => panic!("future is supposed to succeed"),
        }
    }
}
