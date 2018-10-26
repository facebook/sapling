// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Common types used by sibling crates

extern crate failure;
#[macro_use]
extern crate failure_derive;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(all(not(test), feature = "for-tests"))]
extern crate quickcheck;
#[cfg(any(test, feature = "for-tests"))]
extern crate rand;

pub mod errors;
pub mod node;
