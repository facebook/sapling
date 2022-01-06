/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

pub use crate::config::{
    load_common_config, load_repo_configs, load_storage_configs, RepoConfigs, StorageConfigs,
};
pub use crate::errors::ConfigurationError;
pub use convert::Convert;
