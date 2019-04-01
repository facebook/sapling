// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides RepoConfigs structure that can read config from a manifest of a metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]
#![feature(try_from)]

use failure_ext as failure;

pub mod errors;
pub mod repoconfig;

pub use crate::errors::{Error, ErrorKind};
pub use crate::repoconfig::RepoConfigs;
