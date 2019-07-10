#![deny(warnings)]
// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
