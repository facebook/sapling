// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod lz4;

pub use crate::lz4::{compress, compresshc, decompress, decompress_into, decompress_size};
