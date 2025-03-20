/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Utilities interacting with the OS.

// What functions belong here? The theme is similar to mercurial/util.py
//
// Cross platform filesystem / network / process / string / data structures
// utilities that cannot be trivially written using Rust stdlib.
//
// Prefer using the Rust stdlib directly if possible.

pub mod errors;
pub mod file;
pub mod lock;
pub mod path;
pub mod utf8;

pub use fs_err;
