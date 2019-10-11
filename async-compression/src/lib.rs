#![deny(warnings)]
/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

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
