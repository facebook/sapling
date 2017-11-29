// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Framed objects that are only streams, not sinks.

use std::cmp;
use std::io::{self, Read};

use bytes::{BufMut, BytesMut};
use futures::{Async, Poll, Stream};
use tokio_io::AsyncRead;
use tokio_io::codec::Decoder;

const BUFSIZE: usize = 8 * 1024;

#[inline]
pub fn framed_stream<T, Dec>(read: T, decoder: Dec) -> FramedStream<T, Dec>
where
    T: AsyncRead,
    Dec: Decoder,
{
    FramedStream {
        inner: read,
        decoder: decoder,
        // 8KB is a reasonable default
        buf: BytesMut::with_capacity(BUFSIZE),
        eof: false,
        is_readable: false,
    }
}

/// Framed objects that are only streams, not sinks. This is distinct from
/// SplitStream<Framed<T, C>> because that doesn't provide access to `&T` or
/// `&mut T`.
///
/// This was forked from Framed to implement the `into_inner_leading`
/// functionality. https://github.com/alexcrichton/tokio-io/issues/17 tackles
/// implementing this upstream.
#[derive(Debug)]
pub struct FramedStream<T, Dec> {
    inner: T,
    decoder: Dec,
    buf: BytesMut,
    eof: bool,
    is_readable: bool,
}

impl<T, Dec> FramedStream<T, Dec>
where
    T: AsyncRead,
    Dec: Decoder,
{
    #[inline]
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.inner
    }

    #[inline]
    pub fn into_inner_leading(self) -> ReadLeadingBuffer<T> {
        ReadLeadingBuffer {
            inner: self.inner,
            buf: self.buf,
        }
    }
}

impl<T, Dec> Stream for FramedStream<T, Dec>
where
    T: AsyncRead,
    Dec: Decoder,
{
    type Item = Dec::Item;
    type Error = Dec::Error;

    fn poll(&mut self) -> Poll<Option<Dec::Item>, Dec::Error> {
        // This is adapted from Framed::poll in tokio.
        loop {
            // If the read buffer has any pending data, then it could be
            // possible that `decode` will return a new frame. We leave it to
            // the decoder to optimize detecting that more data is required.
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

            // Otherwise, try to read more data and try again. Make sure we've
            // got room to make progress.
            self.buf.reserve(BUFSIZE);
            let got = unsafe {
                // Read 1 byte at a time to avoid over-reading, since we don't know
                // how much we'll need. (Ideally the decoder could tell us how much
                // more input it needs to make progress.)
                // TODO: (jsgf) T23239742 Either fix decode to return amount needed or
                // completely rewrite as a streaming command parser.
                let buf = &mut self.buf;
                let n = {
                    let b = &mut buf.bytes_mut()[..1];

                    self.inner.prepare_uninitialized_buffer(b);
                    let n = try_nb!(self.inner.read(b));
                    assert!(n <= 1);
                    n
                };
                buf.advance_mut(n);
                n
            };
            if got == 0 {
                self.eof = true;
            }

            self.is_readable = true;
        }
    }
}

pub struct ReadLeadingBuffer<T> {
    inner: T,
    buf: BytesMut,
}

impl<T> ReadLeadingBuffer<T> {
    #[inline]
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Read for ReadLeadingBuffer<T>
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buf.len() == 0 {
            return self.inner.read(buf);
        }

        let len = {
            let slice = self.buf.as_ref();
            let len = cmp::min(buf.len(), slice.len());
            if len == 0 {
                return Ok(0);
            }
            let slice = &slice[..len];
            let buf = &mut buf[..len];
            buf.copy_from_slice(slice);
            len
        };

        self.buf.split_to(len);
        Ok(len)
    }
}

impl<T> AsyncRead for ReadLeadingBuffer<T>
where
    T: AsyncRead,
{
}

#[cfg(test)]
mod test {
    use std::io::{Cursor, Write};

    extern crate netstring;
    use self::netstring::NetstringDecoder;

    use tokio_core::reactor;

    use super::*;

    #[test]
    fn simple() {
        let mut core = reactor::Core::new().unwrap();

        let decoder = NetstringDecoder::new();

        let inp = Cursor::new(b"13:hello, world!,".to_vec());
        let stream = framed_stream(inp, decoder);
        let (res, _) = core.run(stream.into_future()).unwrap();

        assert_eq!(res.unwrap().as_ref(), b"hello, world!");
    }

    #[test]
    fn leading_buffer() {
        let mut core = reactor::Core::new().unwrap();

        let decoder = NetstringDecoder::new();

        let inp = Cursor::new(b"13:hello, world!,foo-bar-".to_vec());
        let stream = framed_stream(inp, decoder);
        let (res, mut stream) = core.run(stream.into_future()).unwrap();

        assert_eq!(res.unwrap().as_ref(), b"hello, world!");

        // Make sure only the required input (and no more) has been read.
        assert_eq!(stream.get_ref().position(), 17);

        // Add some more data to the end so that we can test that both the
        // remaining bits in the buffer and the additional data we wrote can be
        // returned.
        assert_matches!(stream.get_mut().write_all(b"baz-quux"), Ok(()));
        stream.get_mut().set_position(25);

        let mut read2 = stream.into_inner_leading();
        let mut out = vec![];
        assert_matches!(read2.read_to_end(&mut out), Ok(0));
        assert!(out.is_empty());
    }
}
