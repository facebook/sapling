// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides RepoConfigs structure that can read config from a manifest of a metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]
#![feature(try_from)]

#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate mercurial;
extern crate mercurial_types;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate toml;
extern crate vfs;

pub mod errors;
pub mod repoconfig;

pub use repoconfig::RepoConfigs;

pub use errors::{Error, ErrorKind};

#[cfg(test)]
extern crate mercurial_types_mocks;
