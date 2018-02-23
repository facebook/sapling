// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(const_fn)]

extern crate failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;

pub mod hash;
pub mod manifest;
pub mod nodehash;
pub mod repo;
