// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides RepoConfigs structure that can read config from a manifest of a metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate mercurial_types;
extern crate vfs;

pub mod errors;
mod repoconfig;

pub use repoconfig::RepoConfigs;

#[cfg(test)]
extern crate mercurial_types_mocks;
