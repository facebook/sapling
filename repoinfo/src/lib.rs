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
// TODO: (sid0) T21726029 tokio/futures deprecated a bunch of stuff, clean it all up
#![allow(deprecated)]

extern crate asyncmemo;
extern crate futures;
extern crate mercurial_types;
#[macro_use]
extern crate heapsize_derive;
extern crate heapsize;

mod gen;
mod nodehashkey;
mod ptrwrap;

pub use ptrwrap::PtrWrap;

pub use gen::{RepoGenCache, Generation};
