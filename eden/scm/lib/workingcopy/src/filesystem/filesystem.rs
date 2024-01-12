/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest_tree::TreeManifest;
use pathmatcher::DynMatcher;
use serde::Serialize;
use termlogger::TermLogger;
use types::HgId;
use types::RepoPathBuf;

#[derive(Debug, Serialize)]
pub enum PendingChange {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
    // Ingored doesn't make sense as a pending change, but in general we don't
    // store info about ignored files, and it is more efficient for the
    // filesystem abstraction to tell us about ignored files as it computes
    // status.
    Ignored(RepoPathBuf),
}

impl PendingChange {
    pub fn get_path(&self) -> &RepoPathBuf {
        match self {
            Self::Changed(path) => path,
            Self::Deleted(path) => path,
            Self::Ignored(path) => path,
        }
    }
}

pub trait FileSystem {
    fn pending_changes(
        &self,
        // The full matcher including user specified filters.
        matcher: DynMatcher,
        // Git ignore matcher, except won't match committed files.
        ignore_matcher: DynMatcher,
        // Directories to always ignore such as ".sl".
        ignore_dirs: Vec<PathBuf>,
        // include ignored files
        include_ignored: bool,
        config: &dyn Config,
        io: &TermLogger,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>>;

    /// Block until potential "status" or "diff" change.
    ///
    /// This function is "correct" if it just returns directly. But that will
    /// trigger potentially slow "status" calculation.
    ///
    /// For supported backends (ex. watchman or edenfs), this function can
    /// watchman subscription or edenfs journal number to delay a prolonged
    /// period of time to optimize unnecessary "status" out.
    fn wait_for_potential_change(&self, config: &dyn Config) -> Result<()> {
        let interval_ms = config.get_or("workingcopy", "poll-interval-ms", || 1000)?;
        std::thread::sleep(Duration::from_millis(interval_ms));
        Ok(())
    }

    fn sparse_matcher(
        &self,
        manifests: &[Arc<TreeManifest>],
        dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>>;

    fn set_parents(
        &self,
        _p1: HgId,
        _p2: Option<HgId>,
        _parent_tree_hash: Option<HgId>,
    ) -> Result<()> {
        Ok(())
    }
}
