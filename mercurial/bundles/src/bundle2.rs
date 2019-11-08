/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Overall coordinator for parsing bundle2 streams.

use std::fmt::{self, Debug, Display, Formatter};
use std::io::{BufRead, Chain, Cursor, Read};
use std::mem;

use bytes::BytesMut;
use futures::{Async, Poll, Stream};

use futures_ext::io::Either;
use futures_ext::BoxFuture;
use slog::Logger;
use tokio_codec::{Framed, FramedParts};
use tokio_io::AsyncRead;

use crate::errors::*;
use crate::part_inner::inner_stream;
use crate::part_outer::{outer_stream, OuterFrame, OuterStream};
use crate::stream_start::StartDecoder;
use crate::Bundle2Item;

pub enum StreamEvent<I, S> {
    Next(I),
    Done(S),
}

impl<I, S> StreamEvent<I, S> {
    pub fn into_next(self) -> ::std::result::Result<I, Self> {
        if let StreamEvent::Next(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

impl<I, S> Debug for StreamEvent<I, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            &StreamEvent::Next(_) => write!(f, "Next(...)"),
            &StreamEvent::Done(_) => write!(f, "Done(...)"),
        }
    }
}

pub type Remainder<R> = (BytesMut, R);

#[derive(Debug)]
pub struct Bundle2Stream<R>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    inner: Bundle2StreamInner,
    current_stream: CurrentStream<R>,
}

#[derive(Debug)]
struct Bundle2StreamInner {
    logger: Logger,
    app_errors: Vec<ErrorKind>,
}

enum CurrentStream<R>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    Start(Framed<R, StartDecoder>),
    Outer(OuterStream<Chain<Cursor<BytesMut>, R>>),
    Inner(BoxFuture<OuterStream<Chain<Cursor<BytesMut>, R>>, Error>),
    Invalid,
    End,
}

impl<R> CurrentStream<R>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    pub fn take(&mut self) -> Self {
        mem::replace(self, CurrentStream::Invalid)
    }
}

impl<R> Display for CurrentStream<R>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    R: AsyncRead + BufRead + Debug + 'static + Send,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
    R: AsyncRead + BufRead + 'static + Send,
{
    pub fn new(logger: Logger, read: R) -> Bundle2Stream<R> {
        Bundle2Stream {
            inner: Bundle2StreamInner {
                logger,
                app_errors: Vec::new(),
            },
            current_stream: CurrentStream::Start(Framed::from_parts(FramedParts::new(
                read,
                StartDecoder,
            ))),
        }
    }

    pub fn app_errors(&self) -> &[ErrorKind] {
        &self.inner.app_errors
    }
}

impl<R> Stream for Bundle2Stream<R>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    type Item = StreamEvent<Bundle2Item, Remainder<R>>;
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
    ) -> (
        Poll<Option<StreamEvent<Bundle2Item, Remainder<R>>>, Error>,
        CurrentStream<R>,
    )
    where
        R: AsyncRead + BufRead + 'static + Send,
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
                        let FramedParts {
                            io,
                            read_buf,
                            write_buf,
                            ..
                        } = stream.into_parts();
                        assert!(
                            write_buf.is_empty(),
                            "write_buf must be empty, since io is not AsyncWrite"
                        );

                        match outer_stream(
                            self.logger.clone(),
                            &start,
                            Cursor::new(read_buf).chain(io),
                        ) {
                            Err(e) => {
                                // Can't do much if reading stream level params
                                // failed -- go to the invalid state.
                                (Err(e.into()), CurrentStream::Invalid)
                            }
                            Ok(v) => {
                                let outer = CurrentStream::Outer(v);
                                (
                                    Ok(Async::Ready(Some(StreamEvent::Next(Bundle2Item::Start(
                                        start,
                                    ))))),
                                    outer,
                                )
                            }
                        }
                    }
                }
            }
            CurrentStream::Outer(mut stream) => {
                match stream.poll() {
                    Err(e) => match e.downcast::<ErrorKind>() {
                        Ok(ek) => {
                            if ek.is_app_error() {
                                self.app_errors.push(ek);
                                self.poll_next(CurrentStream::Outer(stream))
                            } else {
                                (Err(Error::from(ek)), CurrentStream::Outer(stream))
                            }
                        }
                        Err(e) => (Err(e), CurrentStream::Outer(stream)),
                    },
                    Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Outer(stream)),
                    Ok(Async::Ready(None)) => {
                        (Ok(Async::Ready(None)), CurrentStream::Outer(stream))
                    }
                    Ok(Async::Ready(Some(OuterFrame::Header(header)))) => {
                        let (bundle2item, remainder) =
                            inner_stream(self.logger.clone(), header, stream);
                        (
                            Ok(Async::Ready(Some(StreamEvent::Next(bundle2item)))),
                            CurrentStream::Inner(remainder),
                        )
                    }
                    Ok(Async::Ready(Some(OuterFrame::Discard))) => {
                        self.poll_next(CurrentStream::Outer(stream))
                    }
                    Ok(Async::Ready(Some(OuterFrame::StreamEnd))) => {
                        // No more parts to go.
                        let FramedParts {
                            io,
                            mut read_buf,
                            write_buf,
                            ..
                        } = stream.into_parts();
                        assert!(
                            write_buf.is_empty(),
                            "write_buf must be empty, since io is not AsyncWrite"
                        );

                        let io = match io {
                            Either::A(io) => io,
                            Either::B(decompressor) => decompressor.into_inner(),
                        };

                        let (cursor, io) = io.into_inner();
                        read_buf
                            .extend_from_slice(&cursor.get_ref()[(cursor.position() as usize)..]);

                        (
                            Ok(Async::Ready(Some(StreamEvent::Done((read_buf, io))))),
                            CurrentStream::End,
                        )
                    }
                    _ => panic!("Expected a header or StreamEnd!"),
                }
            }
            CurrentStream::Inner(mut remainder) => match remainder.poll() {
                Err(e) => (Err(e), CurrentStream::Invalid),
                Ok(Async::NotReady) => (Ok(Async::NotReady), CurrentStream::Inner(remainder)),
                Ok(Async::Ready(remainder)) => self.poll_next(CurrentStream::Outer(remainder)),
            },
            CurrentStream::Invalid => (
                Err(ErrorKind::Bundle2Decode("corrupt byte stream".into()).into()),
                CurrentStream::Invalid,
            ),
            CurrentStream::End => (Ok(Async::Ready(None)), CurrentStream::End),
        }
    }
}
