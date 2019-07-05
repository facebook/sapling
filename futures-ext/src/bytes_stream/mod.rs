// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod bytes_stream_future;

use std::cmp;
use std::io::{self, BufRead, Read};

use bytes::{BufMut, Bytes, BytesMut};
use futures::{try_ready, Async, Poll, Stream};
use tokio_io::codec::Decoder;
use tokio_io::AsyncRead;

pub use self::bytes_stream_future::BytesStreamFuture;

// 8KB is a reasonable default
const BUFSIZE: usize = 8 * 1024;

#[derive(Debug)]
pub struct BytesStream<S> {
    bytes: BytesMut,
    stream: S,
    stream_done: bool,
}

impl<S: Stream<Item = Bytes>> BytesStream<S> {
    pub fn new(stream: S) -> Self {
        BytesStream {
            bytes: BytesMut::with_capacity(BUFSIZE),
            stream,
            stream_done: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty() && self.stream_done
    }

    pub fn into_parts(self) -> (Bytes, S) {
        (self.bytes.freeze(), self.stream)
    }

    pub fn into_future_decode<Dec>(self, decoder: Dec) -> BytesStreamFuture<S, Dec>
    where
        Dec: Decoder,
        Dec::Error: From<S::Error>,
    {
        BytesStreamFuture::new(self, decoder)
    }

    pub fn prepend_bytes(&mut self, bytes: Bytes) {
        let mut bytes_mut = match bytes.try_mut() {
            Ok(bytes_mut) => bytes_mut,
            Err(bytes) => {
                let cap = cmp::max(BUFSIZE, bytes.len() + self.bytes.len());
                let mut bytes_mut = BytesMut::with_capacity(cap);
                bytes_mut.put(bytes);
                bytes_mut
            }
        };

        bytes_mut.put(&self.bytes);
        self.bytes = bytes_mut;
    }

    fn poll_buffer(&mut self) -> Poll<(), S::Error> {
        if !self.stream_done {
            let bytes = try_ready!(self.stream.poll());
            match bytes {
                None => self.stream_done = true,
                Some(bytes) => self.bytes.extend_from_slice(&bytes),
            }
        }

        Ok(Async::Ready(()))
    }

    fn poll_buffer_until(&mut self, len: usize) -> Poll<(), S::Error> {
        while self.bytes.len() < len && !self.stream_done {
            try_ready!(self.poll_buffer());
        }

        Ok(Async::Ready(()))
    }
}

impl<S: Stream<Item = Bytes>> From<S> for BytesStream<S> {
    fn from(stream: S) -> Self {
        BytesStream::new(stream)
    }
}

impl<S> Read for BytesStream<S>
where
    S: Stream<Item = Bytes, Error = io::Error>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let r#async = self.poll_buffer_until(buf.len())?;
        if self.bytes.is_empty() && r#async.is_not_ready() {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "inner stream not ready",
            ))
        } else {
            let len = {
                let slice = self.bytes.as_ref();
                let len = cmp::min(buf.len(), slice.len());
                if len == 0 {
                    return Ok(0);
                }
                let slice = &slice[..len];
                let buf = &mut buf[..len];
                buf.copy_from_slice(slice);
                len
            };

            self.bytes.split_to(len);
            Ok(len)
        }
    }
}

impl<S> AsyncRead for BytesStream<S> where S: Stream<Item = Bytes, Error = io::Error> {}

impl<S> BufRead for BytesStream<S>
where
    S: Stream<Item = Bytes, Error = io::Error>,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.bytes.is_empty() && self.poll_buffer_until(1)?.is_not_ready() {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "inner stream not ready",
            ))
        } else {
            Ok(self.bytes.as_ref())
        }
    }

    fn consume(&mut self, amt: usize) {
        self.bytes.split_to(amt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoxStream;
    use crate::StreamExt;
    use futures::stream::iter_ok;

    fn make_reader(in_reads: Vec<Vec<u8>>) -> BytesStream<BoxStream<Bytes, io::Error>> {
        let stream = iter_ok(in_reads.into_iter().map(|v| v.into()));
        BytesStream::new(stream.boxify())
    }

    fn do_read<S>(reader: &mut BytesStream<S>, len_to_read: usize) -> io::Result<Vec<u8>>
    where
        S: Stream<Item = Bytes, Error = io::Error>,
    {
        let mut out = vec![0; len_to_read];
        let len_read = reader.read(&mut out)?;
        out.truncate(len_read);
        Ok(out)
    }

    #[test]
    fn test_read_once() -> io::Result<()> {
        let mut reader = make_reader(vec![vec![1, 2, 3, 4]]);
        let out = do_read(&mut reader, 4)?;
        assert_eq!(out, vec![1, 2, 3, 4]);
        Ok(())
    }

    #[test]
    fn test_read_join() -> io::Result<()> {
        let mut reader = make_reader(vec![vec![1, 2], vec![3, 4]]);
        let out = do_read(&mut reader, 4)?;
        assert_eq!(out, vec![1, 2, 3, 4]);
        Ok(())
    }

    #[test]
    fn test_read_split() -> io::Result<()> {
        let mut reader = make_reader(vec![vec![1, 2, 3, 4]]);
        let out = do_read(&mut reader, 2)?;
        assert_eq!(out, vec![1, 2]);
        let out = do_read(&mut reader, 2)?;
        assert_eq!(out, vec![3, 4]);
        Ok(())
    }

    #[test]
    fn test_read_eof() -> io::Result<()> {
        let mut reader = make_reader(vec![vec![1, 2, 3]]);
        let out = do_read(&mut reader, 4)?;
        assert_eq!(out, vec![1, 2, 3]);
        Ok(())
    }

    #[test]
    fn test_read_no_data() -> io::Result<()> {
        let mut reader = make_reader(vec![vec![1, 2, 3]]);
        let out = do_read(&mut reader, 4)?;
        assert_eq!(out, vec![1, 2, 3]);
        let out = do_read(&mut reader, 1)?;
        assert_eq!(out, vec![]);
        Ok(())
    }
}
