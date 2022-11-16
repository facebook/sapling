/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! All structs and functions that are intended to be used in C/C++ code should be placed in this
//! mode, and all extern functions should have the `rust_` prefix to indicate the implementation of
//! the function is written in Rust. Changes to this mod may need regenerations of the C/C++
//! binding header. To regenerate the binding header, run `./tools/cbindgen.sh`.

mod auxdata;
mod backingstore;
mod cbytes;
mod cfallible;
mod init;
mod request;
mod slice;
mod tests;
mod tree;

pub use auxdata::FileAuxData;
pub use cbytes::CBytes;
pub use cfallible::CFallible;
pub use request::Request;
pub use slice::ByteView;
pub use slice::StringView;
pub use tree::Tree;
