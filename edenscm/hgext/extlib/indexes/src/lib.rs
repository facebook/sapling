// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Cpython's macros are not well behaved when imported individually.
#[macro_use]
extern crate cpython;

pub mod errors;
pub mod nodemap;
mod pybuf;

#[allow(non_camel_case_types)]
pub mod pyext;
