/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Cpython's macros are not well behaved when imported individually.
#[macro_use]
extern crate cpython;

#[allow(non_camel_case_types)]
pub mod pyext;

pub use pyext::init_module;
