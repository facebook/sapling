// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
extern crate chashmap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;

extern crate blobrepo;
extern crate mercurial_types;
extern crate mononoke_types;

mod helpers;

pub mod errors;
pub use errors::ErrorKind;

mod index;
pub use index::ReachabilityIndex;

mod genbfs;
pub use genbfs::GenerationNumberBFS;

mod skiplist;
pub use skiplist::SkiplistIndex;
#[cfg(test)]
pub extern crate async_unit;
#[cfg(test)]
pub extern crate branch_wide;
#[cfg(test)]
pub extern crate linear;
#[cfg(test)]
pub extern crate merge_uneven;
#[cfg(test)]
mod tests;
