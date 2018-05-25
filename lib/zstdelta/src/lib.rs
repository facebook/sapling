#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;

extern crate libc;
extern crate zstd_sys;

mod zstdelta;

pub use zstdelta::{apply, diff};
