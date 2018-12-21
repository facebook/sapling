// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate byteorder;
#[macro_use]
extern crate failure;
extern crate libc;
extern crate lz4_sys;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

mod lz4;

pub use lz4::{compress, compresshc, decompress, decompress_into, decompress_size};
