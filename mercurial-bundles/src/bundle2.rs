// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Overall coordinator for parsing bundle2 streams.

use std::fmt::{self, Debug, Display, Formatter};
use std::io::{BufRead, BufReader};
use std::mem;

use futures::{Async, Poll, Stream};
use slog;

use async_compression::Decompressor;
use futures_ext::{AsyncReadExt, FramedStream, ReadLeadingBuffer, StreamWrapper};
use futures_ext::io::Either;
use tokio_io::AsyncRead;

use Bundle2Item;
use errors::*;
use part_inner::{inner_stream, BoxInnerStream};
use part_outer::{outer_stream, OuterFrame, OuterStream};
use stream_start::StartDecoder;

#[derive(Debug)]
pub struct Bundle2Stream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
{
    inner: Bundle2StreamInner,
    current_stream: CurrentStream<'a, R>,
}

#[derive(Debug)]
struct Bundle2StreamInner {
    logger: slog::Logger,
    app_errors: Vec<ErrorKind>,
}

enum CurrentStream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
{
    Start(FramedStream<R, StartDecoder>),
    Outer(OuterStream<'a, BufReader<ReadLeadingBuffer<R>>>),
    Inner(BoxInnerStream<'a, BufReader<ReadLeadingBuffer<R>>>),
    Invalid,
    End(ReadLeadingBuffer<
        Either<BufReader<ReadLeadingBuffer<R>>, Decompressor<'a, BufReader<ReadLeadingBuffer<R>>>>,
    >),
}

impl<'a, R> CurrentStream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
{
    pub fn take(&mut self) -> Self {
        mem::replace(self, CurrentStream::Invalid)
    }
}

impl<'a, R> Display for CurrentStream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::CurrentStream::*;

        let s = match self {
            &Start(_) => "start",
            &Outer(_) => "outer",
            &Inner(_) => "inner",
            &Invalid => "invalid",
            &End(_) => "end",
        };
        write!(fmt, "{}", s)
    }
}

impl<'a, R> Debug for CurrentStream<'a, R>
where
    R: AsyncRead + BufRead + Debug + 'a,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            &CurrentStream::Start(ref framed_stream) => write!(f, "Start({:?})", framed_stream),
            &CurrentStream::Outer(ref outer_stream) => write!(f, "Outer({:?})", outer_stream),
            // InnerStream currently doesn't implement Debug because
            // part_inner::BoolFuture doesn't implement Debug.
            &CurrentStream::Inner(_) => write!(f, "Inner(inner_stream)"),
            &CurrentStream::Invalid => write!(f, "Invalid"),
            &CurrentStream::End(_) => write!(f, "End"),
        }
    }
}

impl<'a, R> Bundle2Stream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
{
    pub fn new(read: R, logger: slog::Logger) -> Bundle2Stream<'a, R> {
        Bundle2Stream {
            inner: Bundle2StreamInner {
                logger: logger,
                app_errors: Vec::new(),
            },
            current_stream: CurrentStream::Start(read.framed_stream(StartDecoder)),
        }
    }

    pub fn app_errors(&self) -> &[ErrorKind] {
        &self.inner.app_errors
    }

    pub fn into_end(
        self,
    ) -> Option<
        ReadLeadingBuffer<
            Either<
                BufReader<ReadLeadingBuffer<R>>,
                Decompressor<'a, BufReader<ReadLeadingBuffer<R>>>,
            >,
        >,
    > {
        match self.current_stream {
            CurrentStream::End(ret) => Some(ret),
            _ => None,
        }
    }
}

impl<'a, R> Stream for Bundle2Stream<'a, R>
where
    R: AsyncRead + BufRead + 'a,
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
    fn poll_next<'a, R>(
        &mut self,
        current_stream: CurrentStream<'a, R>,
    ) -> (Poll<Option<Bundle2Item>, Error>, CurrentStream<'a, R>)
    where
        R: AsyncRead + BufRead + 'a,
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
                        match outer_stream(
                            &start,
                            BufReader::new(stream.into_inner_leading()),
                            &self.logger,
                        ) {
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
                    Err(e) => match e.downcast::<ErrorKind>() {
                        Ok(ek) => if ek.is_app_error() {
                            self.app_errors.push(ek);
                            self.poll_next(CurrentStream::Outer(stream))
                        } else {
                            (Err(Error::from(ek)), CurrentStream::Outer(stream))
                        },
                        Err(e) => (Err(e), CurrentStream::Outer(stream)),
                    },
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
                        (
                            Ok(Async::Ready(None)),
                            CurrentStream::End(stream.into_inner_leading()),
                        )
                    }
                    _ => panic!("Expected a header or StreamEnd!"),
                }
            }
            CurrentStream::Inner(mut stream) => {
                match stream.poll() {
                    Err(e) => (Err(e), CurrentStream::Inner(stream)),
                    Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Inner(stream)),
                    Ok(Async::Ready(Some(v))) => (
                        Ok(Async::Ready(Some(Bundle2Item::Inner(v)))),
                        CurrentStream::Inner(stream),
                    ),
                    Ok(Async::Ready(None)) => {
                        // This part is now parsed -- go back to the outer stream.
                        let outer =
                            CurrentStream::Outer(stream.into_inner().into_inner().into_inner());
                        self.poll_next(outer)
                    }
                }
            }
            CurrentStream::Invalid => (
                Err(ErrorKind::Bundle2Decode("corrupt byte stream".into()).into()),
                CurrentStream::Invalid,
            ),
            CurrentStream::End(s) => (Ok(Async::Ready(None)), CurrentStream::End(s)),
        }
    }
}
