// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides RepoConfigs structure that can read config from a manifest of a metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]
#![feature(try_from)]

extern crate bookmarks;
extern crate bytes;
extern crate scuba;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
#[cfg(test)]
extern crate maplit;
extern crate metaconfig_types;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_regex;
extern crate sql;
#[cfg(test)]
extern crate tempdir;
extern crate toml;

pub mod errors;
pub mod repoconfig;

pub use errors::{Error, ErrorKind};
pub use repoconfig::RepoConfigs;
