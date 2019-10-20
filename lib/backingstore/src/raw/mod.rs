// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! All structs and functions that are intended to be used in C/C++ code should be placed in this
//! mode, and all extern functions should have the `rust_` prefix to indicate the implementation of
//! the function is written in Rust. Changes to this mod may need regenerations of the C/C++
//! binding header. To regenerate the binding header, run `./tools/cbindgen.sh`.

mod cfallible;
mod tests;

pub use cfallible::CFallible;
