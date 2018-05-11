// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Crate to obtain derived information about repos and changesets within a repo
//!
//! Currently this just provides `RepoGenCache` which lazily computes generation numbers
//! for changesets within a repo.
#![deny(warnings)]
#![deny(missing_docs)]

extern crate asyncmemo;
extern crate changesets;
extern crate failure;
extern crate futures;
extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;

extern crate blobrepo;
extern crate futures_ext;
extern crate mercurial_types;

mod gen;
mod nodehashkey;
mod ptrwrap;

pub use ptrwrap::PtrWrap;

pub use gen::{Generation, RepoGenCache};
