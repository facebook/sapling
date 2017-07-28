// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A no-op decoder that just tracks how many bytes have been read.

use std::io;
use std::io::{Read, Write};
use std::result;

use futures::Poll;
use tokio_io::AsyncWrite;

use raw::{RawDecoder, RawEncoder};

pub struct NoopDecoder<R: Read> {
    inner: R,
}

impl<R: Read> NoopDecoder<R> {
    pub fn new(r: R) -> Self {
        NoopDecoder { inner: r }
    }
}

impl<R: Read> RawDecoder<R> for NoopDecoder<R> {
    #[inline]
    fn get_ref(&self) -> &R {
        &self.inner
    }

    #[inline]
    fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    #[inline]
    fn into_inner(self: Box<Self>) -> R {
        self.inner
    }
}

impl<R: Read> Read for NoopDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

pub struct NoopEncoder<W: Write> {
    inner: W,
}

impl<W: Write> NoopEncoder<W> {
    pub fn new(w: W) -> Self {
        NoopEncoder { inner: w }
    }
}

impl<W> RawEncoder<W> for NoopEncoder<W>
where
    W: AsyncWrite + Send,
{
    #[inline]
    fn try_finish(self: Box<Self>) -> result::Result<W, (Box<RawEncoder<W> + Send>, io::Error)> {
        // No internal buffering, so just return the inner struct
        Ok(self.inner)
    }
}

impl<W: Write> Write for NoopEncoder<W> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: AsyncWrite> AsyncWrite for NoopEncoder<W> {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }
}
