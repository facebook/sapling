/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Metered read and write wrappers, to keep track of total bytes passed through
//! streams.

use std::io::{self, Read, Write};

use bytes::{Buf, BufMut};
use futures::{try_ready, Async, Poll};
use tokio_io::{AsyncRead, AsyncWrite};

/// A reader wrapper that tracks the total number of bytes read through it.
pub struct MeteredRead<R: Read> {
    inner: R,
    total_thru: u64,
}

impl<R: Read> MeteredRead<R> {
    pub fn new(inner: R) -> Self {
        MeteredRead {
            inner,
            total_thru: 0,
        }
    }

    /// Total number of bytes passed through this reader.
    pub fn total_thru(&self) -> u64 {
        self.total_thru
    }

    #[inline]
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    #[inline]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for MeteredRead<R> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read_bytes = self.inner.read(buf)?;
        self.total_thru += read_bytes as u64;
        Ok(read_bytes)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let read_bytes = self.inner.read_to_end(buf)?;
        self.total_thru += read_bytes as u64;
        Ok(read_bytes)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        let read_bytes = self.inner.read_to_string(buf)?;
        self.total_thru += read_bytes as u64;
        Ok(read_bytes)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)?;
        self.total_thru += buf.len() as u64;
        Ok(())
    }
}

impl<R: AsyncRead> AsyncRead for MeteredRead<R> {
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error>
    where
        Self: Sized,
    {
        let read_bytes = try_ready!(self.inner.read_buf(buf));
        self.total_thru += read_bytes as u64;
        Ok(Async::Ready(read_bytes))
    }
}

/// A writer wrapper that tracks the total number of bytes written through it.
pub struct MeteredWrite<W: Write> {
    inner: W,
    total_thru: u64,
}

impl<W: Write> MeteredWrite<W> {
    pub fn new(inner: W) -> Self {
        MeteredWrite {
            inner,
            total_thru: 0,
        }
    }

    /// Total number of bytes written through this writer.
    ///
    /// Note that the inner writer might be buffered. This does not take that
    /// into account, so total_thru will include any internally buffered data.
    pub fn total_thru(&self) -> u64 {
        self.total_thru
    }

    #[inline]
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    #[inline]
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for MeteredWrite<W> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written_bytes = self.inner.write(buf)?;
        self.total_thru += written_bytes as u64;
        Ok(written_bytes)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)?;
        self.total_thru += buf.len() as u64;
        Ok(())
    }

    // Can't implement write_fmt because we can't tell how many bytes are being
    // written. It would be pretty weird for an implementation to define a
    // custom write_fmt, though.
}

impl<W: AsyncWrite> AsyncWrite for MeteredWrite<W> {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }

    #[inline]
    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error>
    where
        Self: Sized,
    {
        let written_bytes = try_ready!(self.inner.write_buf(buf));
        self.total_thru += written_bytes as u64;
        Ok(Async::Ready(written_bytes))
    }
}
