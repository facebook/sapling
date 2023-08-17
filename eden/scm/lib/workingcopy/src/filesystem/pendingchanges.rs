/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use configmodel::Config;
use io::IO;
use pathmatcher::DynMatcher;
use serde::Serialize;
use types::RepoPathBuf;

#[derive(Debug, Serialize)]
pub enum PendingChange {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
}

impl PendingChange {
    pub fn get_path(&self) -> &RepoPathBuf {
        match self {
            Self::Changed(path) => path,
            Self::Deleted(path) => path,
        }
    }
}

pub trait PendingChanges {
    fn pending_changes(
        &self,
        // The full matcher including user specified filters.
        matcher: DynMatcher,
        // Git ignore matcher, except won't match committed files.
        ignore_matcher: DynMatcher,
        // Directories to always ignore such as ".sl".
        ignore_dirs: Vec<PathBuf>,
        last_write: SystemTime,
        config: &dyn Config,
        io: &IO,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>>;
}
