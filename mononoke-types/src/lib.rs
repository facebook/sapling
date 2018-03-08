// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Base types used throughout Mononoke.

#![deny(warnings)]
#![feature(try_from)]
#![feature(const_fn)]

#[cfg(test)]
#[macro_use]
extern crate assert_matches;
extern crate bincode;
#[macro_use]
extern crate failure_ext as failure;
extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;
#[macro_use]
extern crate lazy_static;
#[cfg_attr(test, macro_use)]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;
extern crate serde;
#[macro_use]
extern crate serde_derive;

pub mod errors;
pub mod path;

pub use path::{MPath, MPathElement, RepoPath};
