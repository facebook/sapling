extern crate byteorder;
#[macro_use]
extern crate failure;
extern crate lz4_sys;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

mod lz4;

pub use lz4::{compress, decompress};
