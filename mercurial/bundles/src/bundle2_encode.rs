/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::mem;
use std::vec::IntoIter;

use byteorder::ByteOrder;
use bytes::{BigEndian, Buf, BufMut, Bytes, IntoBuf};
use failure_ext::{bail_err, prelude::*};
use futures::stream::Forward;
use futures::{try_ready, Async, AsyncSink, Future, Poll, Sink, StartSend, Stream};
use futures_ext::io::Either::{self, A as UncompressedRead, B as CompressedRead};
use tokio_codec::FramedWrite;
use tokio_io::AsyncWrite;

use async_compression::{Compressor, CompressorType};

use crate::chunk::{Chunk, ChunkEncoder};
use crate::errors::*;
use crate::part_encode::{PartEncode, PartEncodeBuilder};
use crate::part_header::PartId;
use crate::types::StreamHeader;
use crate::utils::{capitalize_first, get_compression_param, is_mandatory_param};
use mercurial_types::percent_encode;

/// This is a general wrapper around a Sink to prevent closing of the underlying Sink. This is
/// useful when using Sink::send_all, because in addition to writing and flushing the data it also
/// closes the sink, which may result in IO errors if called repeatedly on the same Sink
#[derive(Debug)]
struct NotClosingSink<S> {
    pub inner: S,
}
impl<S: Sink> Sink for NotClosingSink<S> {
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.inner.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete()
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}

/// Builder to generate a bundle2.
pub struct Bundle2EncodeBuilder<W> {
    writer: W,
    header: StreamHeader,
    compressor_type: Option<CompressorType>,
    parts: Vec<PartEncode>,
}

impl<W> Bundle2EncodeBuilder<W>
where
    W: AsyncWrite + Send,
{
    pub fn new(writer: W) -> Self {
        Bundle2EncodeBuilder {
            writer,
            header: StreamHeader {
                m_stream_params: HashMap::new(),
                a_stream_params: HashMap::new(),
            },
            compressor_type: None,
            parts: Vec::new(),
        }
    }

    pub fn add_stream_param(&mut self, key: String, val: String) -> Result<&mut Self> {
        if &key.to_lowercase() == "compression" {
            let msg = "stream compression should be set through set_compressor_type";
            bail_err!(ErrorKind::Bundle2Encode(msg.into()));
        }
        if is_mandatory_param(&key)
            .with_context(|| ErrorKind::Bundle2Encode("stream key is invalid".into()))?
        {
            self.header.m_stream_params.insert(key.to_lowercase(), val);
        } else {
            self.header.a_stream_params.insert(key.to_lowercase(), val);
        }
        Ok(self)
    }

    pub fn add_part(&mut self, part: PartEncodeBuilder) -> &mut Self {
        let part_id = self.parts.len() as PartId;
        self.parts.push(part.build(part_id));
        self
    }

    pub fn set_compressor_type<C: Into<Option<CompressorType>>>(&mut self, ct: C) -> &mut Self {
        self.compressor_type = ct.into();
        self
    }

    pub fn build(self) -> Bundle2Encode<W> {
        let mut mparams = self.header.m_stream_params;

        // The compression type becomes a mandatory param.
        mparams.insert(
            "compression".into(),
            get_compression_param(&self.compressor_type).into(),
        );

        // Build the buffer required for the stream header.
        let mut header_buf: Vec<u8> = Vec::new();

        header_buf.put_slice(b"HG20");
        // Reserve 4 bytes for the length.
        header_buf.put_u32_be(0);
        // Now write out the stream header.

        let params = mparams
            .into_iter()
            .map(|x| (capitalize_first(x.0), x.1))
            .chain(self.header.a_stream_params.into_iter());
        Self::build_stream_params(params, &mut header_buf);

        let header_len = (header_buf.len() - 8) as u32;
        BigEndian::write_u32(&mut header_buf[4..], header_len);

        Bundle2Encode {
            state: EncodeState::Start(StartState {
                writer: self.writer,
                compressor_type: self.compressor_type,
                header_buf: Bytes::from(header_buf).into_buf(),
                parts: self.parts,
            }),
        }
    }

    fn build_stream_params<I>(params: I, header_buf: &mut Vec<u8>)
    where
        I: Iterator<Item = (String, String)>,
    {
        let mut first = true;
        for (key, val) in params {
            if !first {
                header_buf.put(b' ');
            }
            first = false;
            header_buf.put(percent_encode(&key).into_bytes().as_slice());
            header_buf.put(b'=');
            header_buf.put(percent_encode(&val).into_bytes().as_slice());
        }
    }
}

/// A sink that chunks generated by PartEncodes goes into.
type PartSink<W> = NotClosingSink<FramedWrite<Either<W, Compressor<W>>, ChunkEncoder>>;

/// A future to drive writing a part to a sink.
type PartFuture<W> = Forward<PartEncode, PartSink<W>>;

#[derive(Debug)]
enum EncodeState<W>
where
    W: AsyncWrite + 'static,
{
    Start(StartState<W>),
    Part(PartFuture<W>, IntoIter<PartEncode>),
    EndOfStream(PartSink<W>, bool),
    Finish(Compressor<W>),
    Done,
    Invalid,
}

impl<W: AsyncWrite> EncodeState<W> {
    fn take(&mut self) -> Self {
        mem::replace(self, EncodeState::Invalid)
    }
}

pub struct Bundle2Encode<W>
where
    W: AsyncWrite + 'static,
{
    state: EncodeState<W>,
}

#[derive(Debug)]
struct StartState<W>
where
    W: AsyncWrite + 'static,
{
    writer: W,
    compressor_type: Option<CompressorType>,
    header_buf: Cursor<Bytes>,
    parts: Vec<PartEncode>,
}

impl<W> Future for StartState<W>
where
    W: AsyncWrite + Send + 'static,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<(), Error> {
        try_ready!(self.writer.write_buf(&mut self.header_buf));
        if self.header_buf.has_remaining() {
            Ok(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    }
}

impl<W> StartState<W>
where
    W: AsyncWrite + Send,
{
    fn finish(self) -> (IntoIter<PartEncode>, PartSink<W>) {
        assert!(!self.header_buf.has_remaining());
        (
            self.parts.into_iter(),
            NotClosingSink {
                inner: FramedWrite::new(
                    match self.compressor_type {
                        None => UncompressedRead(self.writer),
                        Some(compressor_type) => {
                            CompressedRead(Compressor::new(self.writer, compressor_type))
                        }
                    },
                    ChunkEncoder,
                ),
            },
        )
    }
}

impl<W> Future for Bundle2Encode<W>
where
    W: AsyncWrite + Send,
{
    type Item = W;
    type Error = Error;
    fn poll(&mut self) -> Poll<W, Error> {
        let (ret, state) = Self::poll_next(self.state.take());
        self.state = state;
        ret
    }
}

impl<W> Bundle2Encode<W>
where
    W: AsyncWrite + Send,
{
    fn poll_next(state: EncodeState<W>) -> (Poll<W, Error>, EncodeState<W>) {
        match state {
            EncodeState::Start(mut start_state) => {
                match start_state.poll() {
                    Ok(Async::Ready(())) => {
                        // Writing to the stream header is done. Set up the sink
                        // and remaining parts.
                        let (iter, sink) = start_state.finish();
                        Self::poll_next_part(iter, sink)
                    }
                    Ok(Async::NotReady) => (Ok(Async::NotReady), EncodeState::Start(start_state)),
                    Err(err) => {
                        // Somehow writing out the stream header failed. Not
                        // much to do here unfortunately -- we must abort the future.
                        (Err(err), EncodeState::Invalid)
                    }
                }
            }
            EncodeState::Part(part_fut, iter) => Self::poll_part(part_fut, iter),
            EncodeState::EndOfStream(sink, eos_written) => Self::poll_eos(sink, eos_written),
            EncodeState::Finish(compressor) => Self::poll_finish(CompressedRead(compressor)),
            EncodeState::Done => panic!("polled Bundle2Encode future after it is complete"),
            EncodeState::Invalid => {
                panic!("polled Bundle2Encode future after it returned an error")
            }
        }
    }

    fn poll_part(
        mut part_fut: PartFuture<W>,
        iter: IntoIter<PartEncode>,
    ) -> (Poll<W, Error>, EncodeState<W>) {
        match part_fut.poll() {
            Ok(Async::Ready((_part_encoder, sink))) => {
                // This part is done.
                Self::poll_next_part(iter, sink)
            }
            Ok(Async::NotReady) => {
                // This part is still writing.
                (Ok(Async::NotReady), EncodeState::Part(part_fut, iter))
            }
            Err(err) => {
                // TODO: bail on (some forms of) errors?
                (Err(err), EncodeState::Part(part_fut, iter))
            }
        }
    }

    fn poll_next_part(
        mut iter: IntoIter<PartEncode>,
        sink: PartSink<W>,
    ) -> (Poll<W, Error>, EncodeState<W>) {
        match iter.next() {
            Some(part_enc) => Self::poll_part(part_enc.forward(sink), iter),
            None => Self::poll_eos(sink, false),
        }
    }

    fn poll_eos(mut sink: PartSink<W>, eos_written: bool) -> (Poll<W, Error>, EncodeState<W>) {
        if !eos_written {
            match sink.start_send(Chunk::empty()) {
                Ok(AsyncSink::Ready) => (),
                Ok(AsyncSink::NotReady(_)) => {
                    return (Ok(Async::NotReady), EncodeState::EndOfStream(sink, false));
                }
                Err(err) => return (Err(err), EncodeState::Invalid),
            };
        }

        match sink.poll_complete() {
            Ok(Async::Ready(())) => Self::poll_finish(sink.inner.into_inner()),
            Ok(Async::NotReady) => (Ok(Async::NotReady), EncodeState::EndOfStream(sink, true)),
            Err(err) => (Err(err), EncodeState::Invalid),
        }
    }

    fn poll_finish(compressor: Either<W, Compressor<W>>) -> (Poll<W, Error>, EncodeState<W>) {
        let compressor = match compressor {
            UncompressedRead(inner) => return (Ok(Async::Ready(inner)), EncodeState::Done),
            CompressedRead(inner) => inner,
        };

        match compressor.try_finish() {
            Ok(inner) => (Ok(Async::Ready(inner)), EncodeState::Done),
            Err((compressor, err)) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    (Ok(Async::NotReady), EncodeState::Finish(compressor))
                } else {
                    (
                        Err(Error::from(err)
                            .chain_err(ErrorKind::Bundle2Encode(
                                "error while completing write".into(),
                            ))
                            .into()),
                        EncodeState::Invalid,
                    )
                }
            }
        }
    }
}

/// Ensure that Bundle2Encode is Send.
fn _assert_send() {
    fn _assert<T: Send>(_val: &T) {}

    let builder = Bundle2EncodeBuilder::new(Cursor::new(Vec::new()));
    _assert(&builder);
    _assert(&builder.build());
}
