// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(const_fn)]

extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[cfg(test)]
#[macro_use]
extern crate maplit;

#[cfg(test)]
extern crate async_unit;
extern crate futures_ext;
extern crate mercurial_types;
extern crate mononoke_types;

pub mod errors;
pub mod hash;
pub mod manifest;
pub mod nodehash;
pub mod repo;
