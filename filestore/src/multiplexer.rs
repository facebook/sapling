// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::{format_err, Error};
use futures::{sync::mpsc, Future, Sink, Stream};
use futures_ext::{self, BoxStream, FutureExt, StreamExt};

use crate::spawn;

// NOTE: This buffer size is used by the Multiplexer to let the overall multiplexer make progress
// even if one part is being a little slow. The multiplexer typically has individual tasks running
// that are hashing data, so in practice, this buffer is only useful to smooth out one task hitting
// a bit of latency for some reason. It doesn't need to be very large (in fact, empirically, I've
// observed that it needs to be very small), and it shouldn't be mistaken with the Filestore's
// upload concurrency, which is how many upload tasks will be running concurrently (those are tasks
// that perform I/O); those tasks are spawned by prepare.rs, and their number is controlled by the
// Filestore's upload concurrency level.
const BUFFER_SIZE: usize = 5;

type InnerSink<T> = dyn Sink<SinkItem = T, SinkError = mpsc::SendError<T>> + Send + 'static;

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
    pub fn add<I, E, F, B>(
        &mut self,
        builder: B,
    ) -> impl Future<Item = I, Error = spawn::SpawnError<E>>
    where
        I: Send + 'static,
        E: Send + 'static,
        F: Future<Item = I, Error = E> + Send + 'static,
        B: FnOnce(BoxStream<T, !>) -> F,
    {
        let (sender, receiver) = mpsc::channel::<T>(self.buffer_size);

        let sink = match self.sink.take() {
            Some(sink) => Box::new(sink.fanout(sender)) as Box<InnerSink<T>>,
            None => Box::new(sender) as Box<InnerSink<T>>,
        };

        self.sink = Some(sink);

        // NOTE: receiver is a mpsc::Receiver<T>, and those don't actually yield errors (see the
        // source code for Receiver). The std lib doesn't enable the never type, so they used (),
        // but using the never type here is much nicer when dealing with mapping errors from the
        // resulting stream here.
        let receiver = receiver
            .map_err(|_| -> ! {
                unreachable!();
            })
            .boxify();

        // NOTE: We need to start the built future here to make sure it makes progress even if the
        // receiving channel we return here is not polled. This ensures that consumers can't
        // deadlock their Multiplexer.
        // TODO: Pass through an executor
        spawn::spawn_and_start(builder(receiver))
    }

    /// Drain a Stream into the multiplexer.
    pub fn drain<S, E>(self, stream: S) -> impl Future<Item = (), Error = MultiplexerError<E>>
    where
        S: Stream<Item = T, Error = E>,
    {
        let Self { sink, .. } = self;

        match sink {
            // If we have a Sink, then forward the Stream's data into it, and differentiate between
            // errors originating from the Stream and errors originating from the Sink.
            Some(sink) => {
                let sink = sink.sink_map_err(|_| MultiplexerError::Cancelled);
                let stream = stream.map_err(MultiplexerError::InputError);
                sink.send_all(stream).map(|_| ()).left_future()
            }
            // If we have no Sink, then consume the Stream regardless.
            None => stream
                .for_each(|_| Ok(()))
                .map_err(MultiplexerError::InputError)
                .right_future(),
        }
    }
}
