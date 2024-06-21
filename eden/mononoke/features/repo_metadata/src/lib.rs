/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

#[cfg(fbcode_build)]
mod log;
mod process;
mod types;

use std::fmt::Display;

use anyhow::Result;
use metaconfig_types::RepoConfigRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use repo_metadata_checkpoint::RepoMetadataCheckpointRef;

pub use crate::process::repo_metadata_for_bookmark;

pub trait Repo = RepoConfigRef
    + RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + RepoMetadataCheckpointRef
    + Send
    + Sync;

/// Enum determining the mode in which the metadata logger should run
#[derive(Debug, Default, Clone, Copy, clap::ValueEnum, PartialEq, Eq)]
pub enum RepoMetadataLoggerMode {
    /// Run the metadata logger for the entire working copy of the repo
    #[default]
    Full,
    /// Run the metadata logger for the incremental changes in the working copy of the repo
    Incremental,
}

impl Display for RepoMetadataLoggerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            RepoMetadataLoggerMode::Full => write!(f, "full"),
            RepoMetadataLoggerMode::Incremental => write!(f, "incremental"),
        }
    }
}
