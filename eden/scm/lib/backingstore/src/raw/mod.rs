/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! All structs and functions that are intended to be used in C/C++ code should be placed in this
//! mode, and all extern functions should have the `rust_` prefix to indicate the implementation of
//! the function is written in Rust. Changes to this mod may need regenerations of the C/C++
//! binding header. To regenerate the binding header, run `./tools/cbindgen.sh`.

mod backingstore;
mod cbytes;
mod cfallible;
mod tests;
mod tree;

pub use cbytes::CBytes;
pub use cfallible::CFallible;
pub use tree::Tree;
