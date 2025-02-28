/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use context::CoreContext;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use serde::Serialize;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPathBuf;

use crate::client::WorkingCopyClient;

#[derive(Debug, Serialize)]
pub enum PendingChange {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
    // Ignored doesn't make sense as a pending change, but in general we don't
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
        context: &CoreContext,
        // The full matcher including user specified filters.
        matcher: DynMatcher,
        // Git ignore matcher, except won't match committed files.
        ignore_matcher: DynMatcher,
        // Directories to always ignore such as ".sl".
        ignore_dirs: Vec<PathBuf>,
        // include ignored files
        include_ignored: bool,
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

    /// Obtain the TreeState.
    fn get_treestate(&self) -> Result<Arc<Mutex<TreeState>>>;

    /// Get `WorkingCopyClient` for low-level access to the external
    /// working copy manager. Not all filesystem implementations
    /// support this.
    fn get_client(&self) -> Option<Arc<dyn WorkingCopyClient>> {
        None
    }
}
