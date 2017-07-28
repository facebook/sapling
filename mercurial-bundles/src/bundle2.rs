// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Overall coordinator for parsing bundle2 streams.

use std::fmt::{self, Debug, Display, Formatter};
use std::mem;

use futures::{Async, Poll, Stream};
use slog;

use futures_ext::{AsyncReadExt, FramedStream, ReadLeadingBuffer, StreamWrapper};
use tokio_io::AsyncRead;

use Bundle2Item;
use errors::*;
use part_inner::{BoxInnerStream, inner_stream};
use part_outer::{OuterFrame, OuterStream, outer_stream};
use stream_start::StartDecoder;

#[derive(Debug)]
pub struct Bundle2Stream<R>
where
    R: AsyncRead + 'static,
{
    inner: Bundle2StreamInner,
    current_stream: CurrentStream<R>,
}

#[derive(Debug)]
struct Bundle2StreamInner {
    logger: slog::Logger,
    app_errors: Vec<Error>,
}

enum CurrentStream<R>
where
    R: AsyncRead + 'static,
{
    Start(FramedStream<R, StartDecoder>),
    Outer(OuterStream<ReadLeadingBuffer<R>>),
    Inner(BoxInnerStream<ReadLeadingBuffer<R>>),
    Invalid,
    End,
}

impl<R> CurrentStream<R>
where
    R: AsyncRead,
{
    pub fn take(&mut self) -> Self {
        mem::replace(self, CurrentStream::Invalid)
    }
}

impl<R> Display for CurrentStream<R>
where
    R: AsyncRead,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::CurrentStream::*;

        let s = match self {
            &Start(_) => "start",
            &Outer(_) => "outer",
            &Inner(_) => "inner",
            &Invalid => "invalid",
            &End => "end",
        };
        write!(fmt, "{}", s)
    }
}

impl<R> Debug for CurrentStream<R>
where
    R: AsyncRead + 'static + Debug,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            &CurrentStream::Start(ref framed_stream) => write!(f, "Start({:?})", framed_stream),
            &CurrentStream::Outer(ref outer_stream) => write!(f, "Outer({:?})", outer_stream),
            // InnerStream currently doesn't implement Debug because
            // part_inner::BoolFuture doesn't implement Debug.
            &CurrentStream::Inner(_) => write!(f, "Inner(inner_stream)"),
            &CurrentStream::Invalid => write!(f, "Invalid"),
            &CurrentStream::End => write!(f, "End"),
        }
    }
}

impl<R> Bundle2Stream<R>
where
    R: AsyncRead,
{
    pub fn new(read: R, logger: slog::Logger) -> Bundle2Stream<R> {
        Bundle2Stream {
            inner: Bundle2StreamInner {
                logger: logger,
                app_errors: Vec::new(),
            },
            current_stream: CurrentStream::Start(read.framed_stream(StartDecoder)),
        }
    }

    pub fn app_errors(&self) -> &[Error] {
        &self.inner.app_errors
    }
}

impl<R> Stream for Bundle2Stream<R>
where
    R: AsyncRead,
{
    type Item = Bundle2Item;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let current_stream = self.current_stream.take();

        let (ret, stream) = self.inner.poll_next(current_stream);
        self.current_stream = stream;
        ret
    }
}

impl Bundle2StreamInner {
    fn poll_next<R>(
        &mut self,
        current_stream: CurrentStream<R>,
    ) -> (Poll<Option<Bundle2Item>, Error>, CurrentStream<R>)
    where
        R: AsyncRead,
    {
        match current_stream {
            CurrentStream::Start(mut stream) => {
                match stream.poll() {
                    Err(e) => (Err(e), CurrentStream::Start(stream)),
                    Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Start(stream)),
                    Ok(Async::Ready(None)) => {
                        (Ok(Async::Ready(None)), CurrentStream::Start(stream))
                    }
                    Ok(Async::Ready(Some(start))) => {
                        match outer_stream(&start, stream.into_inner_leading(), &self.logger) {
                            Err(e) => {
                                // Can't do much if reading stream level params
                                // failed -- go to the invalid state.
                                (Err(e.into()), CurrentStream::Invalid)
                            }
                            Ok(v) => {
                                let outer = CurrentStream::Outer(v);
                                (Ok(Async::Ready(Some(Bundle2Item::Start(start)))), outer)
                            }
                        }
                    }
                }
            }
            CurrentStream::Outer(mut stream) => {
                match stream.poll() {
                    Err(e) => {
                        if e.is_app_error() {
                            // Don't return these, just continue processing the stream.
                            self.app_errors.push(e);
                            self.poll_next(CurrentStream::Outer(stream))
                        } else {
                            (Err(e), CurrentStream::Outer(stream))
                        }
                    }
                    Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Outer(stream)),
                    Ok(Async::Ready(None)) => {
                        (Ok(Async::Ready(None)), CurrentStream::Outer(stream))
                    }
                    Ok(Async::Ready(Some(OuterFrame::Header(header)))) => {
                        let inner_stream =
                            CurrentStream::Inner(inner_stream(&header, stream, &self.logger));
                        (
                            Ok(Async::Ready(Some(Bundle2Item::Header(header)))),
                            inner_stream,
                        )
                    }
                    Ok(Async::Ready(Some(OuterFrame::Discard))) => {
                        self.poll_next(CurrentStream::Outer(stream))
                    }
                    Ok(Async::Ready(Some(OuterFrame::StreamEnd))) => {
                        // No more parts to go.
                        (Ok(Async::Ready(None)), CurrentStream::End)
                    }
                    _ => panic!("Expected a header or StreamEnd!"),
                }
            }
            CurrentStream::Inner(mut stream) => {
                match stream.poll() {
                    Err(e) => (Err(e), CurrentStream::Inner(stream)),
                    Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Inner(stream)),
                    Ok(Async::Ready(Some(v))) => {
                        (
                            Ok(Async::Ready(Some(Bundle2Item::Inner(v)))),
                            CurrentStream::Inner(stream),
                        )
                    }
                    Ok(Async::Ready(None)) => {
                        // This part is now parsed -- go back to the outer stream.
                        let outer =
                            CurrentStream::Outer(stream.into_inner().into_inner().into_inner());
                        self.poll_next(outer)
                    }
                }
            }
            CurrentStream::Invalid => {
                (
                    Err(
                        ErrorKind::Bundle2Decode("corrupt byte stream".into()).into(),
                    ),
                    CurrentStream::Invalid,
                )
            }
            CurrentStream::End => (Ok(Async::Ready(None)), CurrentStream::End),
        }
    }
}
