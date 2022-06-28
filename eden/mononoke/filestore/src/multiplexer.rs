/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use futures::channel::mpsc;
use futures::future;
use futures::future::Future;
use futures::future::TryFutureExt;
use futures::sink::Sink;
use futures::sink::SinkExt;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use tokio::task::JoinHandle;

// NOTE: This buffer size is used by the Multiplexer to let the overall multiplexer make progress
// even if one part is being a little slow. The multiplexer typically has individual tasks running
// that are hashing data, so in practice, this buffer is only useful to smooth out one task hitting
// a bit of latency for some reason. It doesn't need to be very large (in fact, empirically, I've
// observed that it needs to be very small), and it shouldn't be mistaken with the Filestore's
// upload concurrency, which is how many upload tasks will be running concurrently (those are tasks
// that perform I/O); those tasks are spawned by prepare.rs, and their number is controlled by the
// Filestore's upload concurrency level.
const BUFFER_SIZE: usize = 5;

type InnerSink<T> = dyn Sink<T, Error = mpsc::SendError> + Send + std::marker::Unpin + 'static;

pub struct Multiplexer<T: Send + Sync> {
    buffer_size: usize,
    sink: Option<Box<InnerSink<T>>>,
}

#[derive(Debug)]
pub enum MultiplexerError<E> {
    /// InputError indicates that the input stream hit an error E.
    InputError(E),

    /// Cancelled indicates that one of the consumers add()'ed to this Multiplexer stopped
    /// accepting input (likely because it hit an error).
    Cancelled,
}

impl Into<Error> for MultiplexerError<Error> {
    fn into(self) -> Error {
        use MultiplexerError::*;

        match self {
            InputError(e) => e,
            e @ Cancelled => format_err!("MultiplexerError: {:?}", e),
        }
    }
}

/// Multiplexer lets you attach a set of readers on a Stream, but run them as separate Tokio tasks.
/// In the Filestore, this is used to do hashing of the data Stream on separate tasks.
impl<T: Send + Sync + Clone + 'static> Multiplexer<T> {
    pub fn new() -> Self {
        Self {
            buffer_size: BUFFER_SIZE,
            sink: None,
        }
    }

    /// Add a new consumer on the Multiplexer. To do so, pass a builder that expects a Stream of
    /// data as input (the same data you'll pass in later when draining the Multiplexer), and
    /// retuns a Future. This returns a Future with the same result, and an Error type of
    /// SpawnError, which is a thin wrapper over your Future's Error type.
    pub fn add<I, F, B>(&mut self, builder: B) -> JoinHandle<I>
    where
        I: Send + 'static,
        F: Future<Output = I> + Send + 'static,
        B: FnOnce(mpsc::Receiver<T>) -> F,
    {
        let (sender, receiver) = mpsc::channel::<T>(self.buffer_size);

        let sink = match self.sink.take() {
            Some(sink) => Box::new(sink.fanout(sender)) as Box<InnerSink<T>>,
            None => Box::new(sender) as Box<InnerSink<T>>,
        };

        self.sink = Some(sink);

        // NOTE: We need to start the built future here to make sure it makes progress even if the
        // receiving channel we return here is not polled. This ensures that consumers can't
        // deadlock their Multiplexer.
        // TODO: Pass through an executor?
        tokio::task::spawn(builder(receiver))
    }

    /// Drain a Stream into the multiplexer.
    pub async fn drain<'a, S, E>(self, stream: S) -> Result<(), MultiplexerError<E>>
    where
        S: Stream<Item = Result<T, E>> + Send,
    {
        let Self { sink, .. } = self;

        match sink {
            // If we have a Sink, then forward the Stream's data into it, and differentiate between
            // errors originating from the Stream and errors originating from the Sink.
            Some(sink) => {
                let mut sink = sink.sink_map_err(|_| MultiplexerError::Cancelled);
                // NOTE: I'd like to use futures::pin_mut! here, but:
                // https://github.com/rust-lang/rust/issues/64552
                let mut stream = stream.map_err(MultiplexerError::InputError).boxed();
                sink.send_all(&mut stream).await
            }
            // If we have no Sink, then consume the Stream regardless.
            None => {
                stream
                    .try_for_each(|_| future::ready(Ok(())))
                    .map_err(MultiplexerError::InputError)
                    .await
            }
        }
    }
}
