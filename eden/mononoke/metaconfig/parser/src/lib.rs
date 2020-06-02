/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Loading and parsing of Mononoke configuration.

#![deny(missing_docs)]
#![deny(warnings)]

pub mod config;
mod convert;
pub mod errors;
mod raw;

pub use crate::config::RepoConfigs;
pub use crate::errors::ErrorKind;
