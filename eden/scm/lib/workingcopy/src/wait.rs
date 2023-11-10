/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use configmodel::Config;
use io::IO;
use status::FileStatus;
use types::hgid::NULL_ID;
use types::RepoPathBuf;

use crate::metadata::Metadata;
use crate::workingcopy::WorkingCopy;

/// State to detect working copy changes.
///
/// Internally this tracks the "Metadata" of affected files
/// to detect content changes.
pub struct Wait {
    dot_dir: PathBuf,
    treestate_wait: treestate::Wait,
    // "Status" with metadata to detect content changes.
    metadata_map: HashMap<RepoPathBuf, (FileStatus, Option<Metadata>)>,
}

/// Output of `wait_for_change`.
#[derive(Copy, Clone, Debug)]
pub enum WaitOutput {
    ShouldReload,
    Changed,
}

impl Wait {
    /// Construct a `Wait` to detect changes.
    /// Under the hood, stat all files in `status` output.
    pub fn new(wc: &WorkingCopy, dot_dir: &Path, config: &dyn Config) -> anyhow::Result<Self> {
        let treestate_wait = treestate::Wait::from_dot_dir(dot_dir);
        let matcher = Arc::new(pathmatcher::AlwaysMatcher::new());
        let list_ignored = false;
        let io = IO::null();

        let status = wc.status(matcher, SystemTime::UNIX_EPOCH, list_ignored, config, &io)?;

        // Collect metadata of all changed files.
        let vfs = wc.vfs();
        let mut metadata_map = HashMap::new();
        for (path, file_status) in status.iter() {
            // PERF: Right now we stat all files manually because it's easier to do so with
            // the exiting API. For optimal performance we might want to use watchman (or EdenFS)
            // provided metadata directly.
            let metadata: Option<Metadata> = vfs.metadata(path).ok().map(Into::into);
            metadata_map.insert(path.to_owned(), (file_status, metadata));
        }

        Ok(Self {
            dot_dir: dot_dir.to_owned(),
            treestate_wait,
            metadata_map,
        })
    }

    /// Wait for `status` or content (`diff`) changes.
    ///
    /// Returns `Ok(WaitOutput::Changed)` if changes are detected.
    ///
    /// Returns `Ok(WaitOutput::ShouldReload)` if the callsite should reload
    /// `WorkingCopy` from disk to pick up new changes. The callsite should
    /// preserve the `Wait` state.
    pub fn wait_for_change(
        &mut self,
        wc: &WorkingCopy,
        config: &dyn Config,
    ) -> anyhow::Result<WaitOutput> {
        // Note: this check updates `treestate_wait` so the notification is sent
        // out only once.
        if self.treestate_wait.is_dirstate_changed() {
            return Ok(WaitOutput::ShouldReload);
        }

        // Defensive check. In case the callsite does not reload WorkingCopy
        // after receiving ShouldReload sent (once) above, send another
        // ShouldReload if we detect an obvious (p1) mismatch.
        //
        // This does not cover all possible race conditions. "wait_for_change"
        // does not expose the detailed "status", so it might be good enough.
        //
        // A more "accurate" check would be checking the treestate filename,
        // root, etc. But that has complexities about the edenfs dirstate,
        // which does not yet use a real on-disk treestate.
        if wc.parents()?.first().unwrap_or(&NULL_ID) != &self.treestate_wait.p1() {
            return Ok(WaitOutput::ShouldReload);
        }

        loop {
            let new_wait = Self::new(wc, &self.dot_dir, config)?;
            if new_wait.metadata_map == self.metadata_map {
                // Not changed.
                wc.filesystem.lock().wait_for_potential_change(config)?;
            } else {
                *self = new_wait;
                break;
            }
        }

        Ok(WaitOutput::Changed)
    }
}

impl WaitOutput {
    /// Returns `true` if the working copy should be reloaded.
    pub fn should_reload_working_copy(&self) -> bool {
        matches!(self, Self::ShouldReload)
    }
}
