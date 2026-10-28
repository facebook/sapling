/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Links every Rust library that EdenFS calls through cxx into the one static
//! library that the CMake build links. Separate static libraries would each
//! carry their own build of the cxx runtime, and those collide at link time.

pub use backingstore;
pub use backtrace_ffi;
pub use edenfs_ffi;
pub use manifold_ffi;
pub use redirect_ffi;
