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
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
#[cfg(test)]
extern crate maplit;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg(test)]
extern crate tempdir;
extern crate toml;

extern crate mercurial;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate mononoke_types;

pub mod errors;
pub mod repoconfig;

pub use repoconfig::{CacheWarmupParams, LfsParams, ManifoldArgs, PushrebaseParams,
                     RemoteBlobstoreArgs, RepoConfigs, RepoReadOnly, RepoType};

pub use errors::{Error, ErrorKind};
