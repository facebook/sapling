// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;

extern crate libc;
extern crate zstd_sys;

mod zstdelta;

pub use crate::zstdelta::{apply, diff};
