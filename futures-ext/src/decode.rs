// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A layered `Decoder` adapter for `Stream` transformations
//!
//! This module implements an adapter to allow a `tokio_io::codec::Decoder` implementation
//! to transform a `Stream` - specifically, decode from a `Stream` of `Bytes` into some
//! structured type.
//!
//! This allows multiple protocols to be layered and composed with operations on `Streams`,
//! rather than restricting all codec operations to `AsyncRead`/`AsyncWrite` operations on
//! an underlying transport.

use bytes::{BufMut, Bytes, BytesMut};
use futures::{try_ready, Async, Poll, Stream};
use tokio_io::codec::Decoder;

use crate::{BoxStreamWrapper, StreamWrapper};

pub fn decode<In, Dec>(input: In, decoder: Dec) -> LayeredDecode<In, Dec>
where
    In: Stream<Item = Bytes>,
    Dec: Decoder,
{
    LayeredDecode {
        input,
        decoder,
        // 8KB is a reasonable default
        buf: BytesMut::with_capacity(8 * 1024),
        eof: false,
        is_readable: false,
    }
}

#[derive(Debug)]
pub struct LayeredDecode<In, Dec> {
    input: In,
    decoder: Dec,
    buf: BytesMut,
    eof: bool,
    is_readable: bool,
}

impl<In, Dec> Stream for LayeredDecode<In, Dec>
where
    In: Stream<Item = Bytes>,
    Dec: Decoder,
    Dec::Error: From<In::Error>,
{
    type Item = Dec::Item;
    type Error = Dec::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Dec::Error> {
        // This is adapted from Framed::poll in tokio. This does its own thing
        // because converting the Bytes input stream to an Io object and then
        // running it through Framed is pointless.
        loop {
            if self.is_readable {
                if self.eof {
                    let ret = if self.buf.len() == 0 {
                        None
                    } else {
                        self.decoder.decode_eof(&mut self.buf)?
                    };
                    return Ok(Async::Ready(ret));
                }
                if let Some(frame) = self.decoder.decode(&mut self.buf)? {
                    return Ok(Async::Ready(Some(frame)));
                }
                self.is_readable = false;
            }

            assert!(!self.eof);

            match try_ready!(self.input.poll()) {
                Some(v) => {
                    self.buf.reserve(v.len());
                    self.buf.put(v);
                }
                None => self.eof = true,
            }

            self.is_readable = true;
        }
    }
}

impl<In, Dec> StreamWrapper<In> for LayeredDecode<In, Dec>
where
    In: Stream<Item = Bytes>,
{
    fn into_inner(self) -> In {
        // TODO: do we want to check that buf is empty? otherwise we might lose data
        self.input
    }
}

impl<In, Dec> BoxStreamWrapper<In> for LayeredDecode<In, Dec>
where
    In: Stream<Item = Bytes>,
{
    #[inline]
    fn get_ref(&self) -> &In {
        &self.input
    }

    #[inline]
    fn get_mut(&mut self) -> &mut In {
        &mut self.input
    }

    fn into_inner(self: Box<Self>) -> In {
        // TODO: do we want to check that buf is empty? otherwise we might lose data
        self.input
    }
}

#[cfg(test)]
mod test {
    use netstring::NetstringDecoder;

    use std::io;

    use bytes::Bytes;
    use futures::{stream, Stream};

    use super::*;

    #[test]
    fn simple() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let decoder = NetstringDecoder::new();

        let inp = stream::iter_ok::<_, io::Error>(vec![Bytes::from(&b"13:hello, world!,"[..])]);

        let dec = decode(inp, decoder);
        let out = Vec::new();

        let xfer = dec
            .map_err(|err| -> () {
                panic!("bad = {}", err);
            })
            .forward(out);

        let (_, out) = runtime.block_on(xfer).unwrap();
        let out = out
            .into_iter()
            .flat_map(|x| x.as_ref().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(out, b"hello, world!");
    }

    #[test]
    fn large() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let decoder = NetstringDecoder::new();

        let inp = stream::iter_ok::<_, io::Error>(vec![Bytes::from(
            "13:hello, world!,".repeat(5000).as_bytes(),
        )]);

        let dec = decode(inp, decoder);
        let out = Vec::new();

        let xfer = dec
            .map_err(|err| -> () {
                panic!("bad = {}", err);
            })
            .forward(out);

        let (_, out) = runtime.block_on(xfer).unwrap();
        let out = out
            .into_iter()
            .flat_map(|x| x.as_ref().to_vec())
            .collect::<Vec<_>>();

        assert_eq!(out, "hello, world!".repeat(5000).as_bytes());
    }

    #[test]
    fn partial() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let decoder = NetstringDecoder::new();

        let inp = stream::iter_ok::<_, io::Error>(vec![
            Bytes::from(&b"13:hel"[..]),
            Bytes::from(&b"lo, world!,"[..]),
        ]);

        let dec = decode(inp, decoder);
        let out = Vec::new();

        let xfer = dec
            .map_err(|err| -> () {
                panic!("bad = {}", err);
            })
            .forward(out);

        let (_, out) = runtime.block_on(xfer).unwrap();
        let out = out
            .into_iter()
            .flat_map(|x| x.as_ref().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(out, b"hello, world!");
    }
}
