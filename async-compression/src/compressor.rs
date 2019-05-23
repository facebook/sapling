// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Non-blocking, buffered compression.

use std::fmt::{self, Debug, Formatter};
use std::io::{self, Write};
use std::result;

use bzip2;
use bzip2::write::BzEncoder;
use flate2;
use flate2::write::GzEncoder;
use futures::Poll;
use tokio_io::AsyncWrite;

use crate::decompressor::DecompressorType;
use crate::raw::{AsyncZstdEncoder, RawEncoder};
use crate::retry::retry_write;

#[derive(Clone, Copy, Debug)]
pub enum CompressorType {
    Bzip2(bzip2::Compression),
    Gzip(flate2::Compression),
    Zstd { level: i32 },
}

impl CompressorType {
    pub fn decompressor_type(&self) -> DecompressorType {
        match self {
            &CompressorType::Bzip2(_) => DecompressorType::Bzip2,
            &CompressorType::Gzip(_) => DecompressorType::Gzip,
            &CompressorType::Zstd { .. } => DecompressorType::OverreadingZstd,
        }
    }
}

pub struct Compressor<W>
where
    W: AsyncWrite + 'static,
{
    c_type: CompressorType,
    inner: Box<dyn RawEncoder<W> + Send>,
}

impl<W> Compressor<W>
where
    W: AsyncWrite + Send + 'static,
{
    pub fn new(w: W, ct: CompressorType) -> Self {
        Compressor {
            c_type: ct,
            inner: match ct {
                CompressorType::Bzip2(level) => Box::new(BzEncoder::new(w, level)),
                CompressorType::Gzip(level) => Box::new(GzEncoder::new(w, level)),
                CompressorType::Zstd { level } => Box::new(AsyncZstdEncoder::new(w, level)),
            },
        }
    }

    pub fn try_finish(self) -> result::Result<W, (Self, io::Error)> {
        match self.inner.try_finish() {
            Ok(writer) => Ok(writer),
            Err((encoder, e)) => Err((
                Compressor {
                    c_type: self.c_type,
                    inner: encoder,
                },
                e,
            )),
        }
    }
}

impl<W> Write for Compressor<W>
where
    W: AsyncWrite + Send,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        retry_write(self.inner.by_ref(), buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W> AsyncWrite for Compressor<W>
where
    W: AsyncWrite + Send,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }
}

impl<W: AsyncWrite> Debug for Compressor<W> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compressor")
            .field("c_type", &self.c_type)
            .finish()
    }
}

/// Ensure that compressors implement Send.
fn _assert_send() {
    use std::io::Cursor;

    fn _assert<T: Send>(_val: T) {}

    _assert(Compressor::new(
        Cursor::new(Vec::new()),
        CompressorType::Bzip2(bzip2::Compression::Default),
    ));
}
