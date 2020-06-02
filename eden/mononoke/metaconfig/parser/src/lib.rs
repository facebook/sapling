/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides RepoConfigs structure that can read config from a manifest of a metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

mod convert;
pub mod errors;
pub mod repoconfig;

pub use crate::errors::ErrorKind;
pub use crate::repoconfig::RepoConfigs;
