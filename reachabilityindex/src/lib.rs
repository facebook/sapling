// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure_ext as failure;
extern crate futures;

extern crate blobrepo;
extern crate mercurial_types;

mod index;
pub use index::ReachabilityIndex;
