#![deny(warnings)]
// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[macro_use]
#[cfg(test)]
extern crate assert_matches;
extern crate bytes;
extern crate bzip2;
extern crate flate2;
#[macro_use]
extern crate futures;
#[macro_use]
#[cfg(test)]
extern crate quickcheck;
#[cfg(test)]
extern crate tokio;
extern crate tokio_io;
extern crate zstd;

mod compressor;
mod decompressor;
pub mod membuf;
pub mod metered;
mod raw;
mod retry;

#[cfg(test)]
mod test;

pub use crate::compressor::{Compressor, CompressorType};
pub use crate::decompressor::{Decompressor, DecompressorType};

pub use bzip2::Compression as Bzip2Compression;
pub use flate2::Compression as FlateCompression;
