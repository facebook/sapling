// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Non-blocking, buffered compression and decompression

use std::fmt::{self, Debug, Formatter};
use std::io::{self, BufRead, Read};

use bzip2::bufread::BzDecoder;
use flate2::bufread::GzDecoder;
use tokio_io::AsyncRead;
use zstd::Decoder as ZstdDecoder;

use crate::raw::RawDecoder;

pub struct Decompressor<'a, R>
where
    R: AsyncRead + BufRead + 'a + Send,
{
    d_type: DecompressorType,
    inner: Box<dyn RawDecoder<R> + 'a + Send>,
}

#[derive(Clone, Copy, Debug)]
pub enum DecompressorType {
    Bzip2,
    Gzip,
    /// The Zstd Decoder is overreading it's input. Consider this situation: you have a Reader that
    /// returns parts of it's data compressed with Zstd and the remainder decompressed. Gzip and
    /// Bzip2 will consume only the compressed bytes leaving the remainder untouched. The Zstd
    /// though will consume some of the decomressed bytes, so that once you call `::into_inner()`
    /// on it, the returned Reader will not contain the decomressed bytes.
    ///
    /// Advice: use only if the entire Reader content needs to be decompressed
    /// You have been warned
    OverreadingZstd,
}

impl<'a, R> Decompressor<'a, R>
where
    R: AsyncRead + BufRead + 'a + Send,
{
    pub fn new(r: R, dt: DecompressorType) -> Self {
        Decompressor {
            d_type: dt,
            inner: match dt {
                DecompressorType::Bzip2 => Box::new(BzDecoder::new(r)),
                DecompressorType::Gzip => Box::new(GzDecoder::new(r)),
                DecompressorType::OverreadingZstd => Box::new(
                    ZstdDecoder::with_buffer(r).expect("ZstdDecoder failed to create. Are we OOM?"),
                ),
            },
        }
    }

    #[inline]
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref()
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut()
    }

    #[inline]
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<'a, R: AsyncRead + BufRead + 'a + Send> Read for Decompressor<'a, R> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<'a, R: AsyncRead + BufRead + 'a + Send> AsyncRead for Decompressor<'a, R> {}

impl<'a, R: AsyncRead + BufRead + 'a + Send> Debug for Decompressor<'a, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decompressor")
            .field("decoder_type", &self.d_type)
            .finish()
    }
}
