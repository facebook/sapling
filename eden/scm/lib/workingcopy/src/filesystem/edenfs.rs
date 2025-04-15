/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use configmodel::Config;
use configmodel::ConfigExt;
use context::CoreContext;
use edenfs_client::EdenFsClient;
use edenfs_client::FileStatus;
use fs_err::File;
use fs_err::remove_file;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use storemodel::FileStore;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;
use types::hgid::NULL_ID;
use vfs::VFS;

use crate::client::WorkingCopyClient;
use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;
use crate::util::added_files;

enum DeraceMode {
    Off,
    // timeout, fatal?
    On(Duration, bool),
}

pub struct EdenFileSystem {
    treestate: Arc<Mutex<TreeState>>,
    client: Arc<EdenFsClient>,
    vfs: VFS,
    store: Arc<dyn FileStore>,

    // For wait_for_potential_change
    journal_position: Cell<(i64, i64)>,

    derace_mode: DeraceMode,
}

impl EdenFileSystem {
    pub fn new(
        config: &dyn Config,
        client: Arc<EdenFsClient>,
        vfs: VFS,
        dot_dir: &Path,
        store: Arc<dyn FileStore>,
    ) -> Result<Self> {
        let journal_position = Cell::new(client.get_journal_position()?);
        let treestate = create_treestate(dot_dir, vfs.case_sensitive())?;
        let treestate = Arc::new(Mutex::new(treestate));

        let derace_mode = if cfg!(windows) {
            let timeout: Duration = config
                .get_or("experimental", "derace-eden-status-timeout", || {
                    Duration::from_secs(3)
                })
                .map_err(|err| anyhow!(err))?;
            match config
                .get("experimental", "derace-eden-status-mode")
                .as_deref()
            {
                Some("info") => DeraceMode::On(timeout, false),
                Some("fatal") => DeraceMode::On(timeout, true),
                _ => DeraceMode::Off,
            }
        } else {
            DeraceMode::Off
        };

        Ok(EdenFileSystem {
            treestate,
            client,
            vfs,
            store,
            journal_position,
            derace_mode,
        })
    }

    fn get_status(
        &self,
        p1: HgId,
        include_ignored: bool,
    ) -> Result<BTreeMap<RepoPathBuf, FileStatus>> {
        // If "derace" mode is enabled, below we touch a file and then keep calling eden
        // status until we see the (untracked) file in the results. This is to mitigate a
        // ProjectedFS race condition on Windows where eden's reported status won't
        // reflect recent fs writes until eden receives the corresponding (async)
        // notification from ProjectedFS.

        // Use a static touch file path to avoid creating too many files in the overlay.
        // Note that this means we are open to race conditions between two concurrent
        // "status" calls sharing the same touch file, but that should be rare (and the
        // worst case is we just don't perform the derace check).
        let derace_touch_file: &RepoPath =
            ".eden-status-derace-GSZULQFGEEJXIONP".try_into().unwrap();

        // This is set iff we create the touch file.
        let mut wait_for_touch_file: Option<Duration> = None;
        let mut propagate_derace_error = false;

        if let DeraceMode::On(timeout, fatal) = self.derace_mode {
            let touch_path = self.vfs.join(derace_touch_file);
            // Note: this touch file approach will be ineffective if the touch file
            // already exists. The assumption is that will be very rare. We clean up the
            // touch file aggressively below.
            if let Err(err) = File::create(&touch_path) {
                tracing::trace!(target: "eden_derace_info", eden_derace_error="error creating");
                tracing::error!(?err, %derace_touch_file, "error writing derace touch file");
            } else {
                tracing::trace!("wrote derace touch file");
                wait_for_touch_file = Some(timeout);
                propagate_derace_error = fatal;
            }
        }

        let mut start_time: Option<Instant> = None;
        loop {
            let mut status_map = self.client.get_status(p1, include_ignored)?;

            // Handle derace touch file regardless of whether we created it. We want to
            // ignore it and clean it up if it leaked previously.
            if status_map.remove(derace_touch_file).is_some() {
                let touch_path = self.vfs.join(derace_touch_file);
                if let Err(err) = remove_file(&touch_path) {
                    tracing::trace!(target: "eden_derace_info", eden_derace_error="error removing");
                    tracing::error!(?err, %derace_touch_file, "error removing derace touch file");
                }

                if wait_for_touch_file.is_some() {
                    // If we are in derace mode, log how long we waited.
                    match start_time {
                        Some(start) => {
                            // We had multiple loops - log additional time we waited past first "status".
                            tracing::trace!(elapsed=?start.elapsed(), "eventually found derace touch file");
                            tracing::trace!(target: "eden_derace_info", eden_derace_elapsed=start.elapsed().as_millis());
                        }
                        None => {
                            // We saw touch file on first status - log "0".
                            tracing::trace!("found derace touch file on first try");
                            tracing::trace!(target: "eden_derace_info", eden_derace_elapsed=0);
                        }
                    }
                }

                return Ok(status_map);
            }

            let timeout = match wait_for_touch_file {
                Some(timeout) => timeout,
                // We didn't create a touch file - nothing to check or wait for.
                None => return Ok(status_map),
            };

            //
            // From here we know we are in derace mode, and the first status call did not contain the touch file.
            //

            if !self.vfs.exists(derace_touch_file).unwrap_or(false) {
                tracing::warn!("derace touch file unexpectedly missing");
                tracing::trace!(target: "eden_derace_info", eden_derace_error="file missing");

                // Touch file has disappeared from disk - probably raced with another
                // "status" call that cleaned up the touch file. Should be pretty rare, so
                // let's just give up and say "ok".
                return Ok(status_map);
            }

            match start_time {
                // Start the derace clock _after_ the first status attempt (i.e. it measures additional time).
                None => start_time = Some(Instant::now()),
                Some(start) => {
                    if start.elapsed() >= timeout {
                        tracing::trace!(target: "eden_derace_info", eden_derace_error="timeout");

                        if propagate_derace_error {
                            bail!("failed to derace EdenFS status after {:?}", start.elapsed());
                        } else {
                            return Ok(status_map);
                        }
                    }
                }
            }

            // Wait a bit for touch file PJFS notification to get to eden.
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn create_treestate(dot_dir: &std::path::Path, case_sensitive: bool) -> Result<TreeState> {
    let dirstate_path = dot_dir.join("dirstate");
    tracing::trace!("loading edenfs dirstate");
    TreeState::from_overlay_dirstate(&dirstate_path, case_sensitive)
}

impl FileSystem for EdenFileSystem {
    #[tracing::instrument(skip_all)]
    fn pending_changes(
        &self,
        _ctx: &CoreContext,
        matcher: DynMatcher,
        ignore_matcher: DynMatcher,
        _ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let p1 = self
            .treestate
            .lock()
            .parents()
            .next()
            .unwrap_or_else(|| Ok(NULL_ID))?;

        let status_map = self.get_status(p1, include_ignored)?;

        // In rare cases, a file can transition in the dirstate directly from "normal" to
        // "added". Eden won't report a pending change if the file is not modified (since
        // it looks like an unmodified file until dirstate p1 is updated). So, here we
        // look for added files that aren't in the results from Eden. If the files exist
        // on disk, we inject a pending change. Otherwise, later logic in status infers
        // that the added file must have been removed from disk because the file isn't in
        // the pending changes.
        let extra_added_files = added_files(&mut self.treestate.lock())?
            .into_iter()
            .filter_map(|path| {
                if status_map.contains_key(&path) {
                    None
                } else {
                    match self.vfs.exists(&path) {
                        Ok(true) => Some(Ok(PendingChange::Changed(path))),
                        Ok(false) => None,
                        Err(err) => Some(Err(err)),
                    }
                }
            })
            .collect::<Vec<_>>();

        Ok(Box::new(status_map.into_iter().filter_map(
            move |(path, status)| {
                tracing::trace!(target: "workingcopy::filesystem::edenfs::status", %path, ?status, "eden status");
                // EdenFS reports files that are present in the overlay but filtered from the repo
                // as untracked. We "drop" any files that are excluded by the current filter.
                let mut matched = false;
                let result = match matcher.matches_file(&path) {
                    Ok(true) => {
                        matched = true;
                        match &status {
                            FileStatus::Removed => Some(Ok(PendingChange::Deleted(path))),
                            FileStatus::Ignored => Some(Ok(PendingChange::Ignored(path))),
                            FileStatus::Added => {
                                // EdenFS doesn't know about global ignore files in ui.ignore.* config, so we need to run
                                // untracked files through our ignore matcher.
                                match ignore_matcher.matches_file(&path) {
                                    Ok(ignored) => {
                                        if ignored {
                                            if include_ignored {
                                                Some(Ok(PendingChange::Ignored(path)))
                                            } else {
                                                None
                                            }
                                        } else {
                                            Some(Ok(PendingChange::Changed(path)))
                                        }
                                    }
                                    Err(err) => Some(Err(err)),
                                }
                            },
                            FileStatus::Modified => Some(Ok(PendingChange::Changed(path))),
                        }
                    },
                    Ok(false) => None,
                    Err(e) => {
                        tracing::warn!(
                            "failed to determine if {} is ignored or not tracked by the active filter: {:?}",
                            &path,
                            e
                        );
                        Some(Err(e))
                    }
                };

                if tracing::enabled!(tracing::Level::TRACE) {
                    if let Some(result) = &result {
                        let result = result.as_ref().ok();
                        tracing::trace!(%matched, ?result, " processed eden status");
                    }
                }

                result
            },
        ).chain(extra_added_files.into_iter())))
    }

    fn wait_for_potential_change(&self, config: &dyn Config) -> Result<()> {
        let interval_ms = config
            .get_or("workingcopy", "poll-interval-ms-edenfs", || 200)?
            .max(50);
        loop {
            let new_journal_position = self.client.get_journal_position()?;
            let old_journal_position = self.journal_position.get();
            if old_journal_position != new_journal_position {
                tracing::trace!(
                    "edenfs journal position changed: {:?} -> {:?}",
                    old_journal_position,
                    new_journal_position
                );
                self.journal_position.set(new_journal_position);
                break;
            }
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
        Ok(())
    }

    fn sparse_matcher(
        &self,
        manifests: &[Arc<TreeManifest>],
        dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        crate::sparse::sparse_matcher(
            &self.vfs,
            manifests,
            self.store.clone(),
            // XXX: This does not work for dotgit submodule.
            &self.vfs.root().join(dot_dir),
        )
    }

    fn set_parents(
        &self,
        p1: HgId,
        p2: Option<HgId>,
        parent_tree_hash: Option<HgId>,
    ) -> Result<()> {
        let parent_tree_hash =
            parent_tree_hash.context("parent tree required for setting EdenFS parents")?;
        self.client.set_parents(p1, p2, parent_tree_hash)
    }

    fn get_treestate(&self) -> Result<Arc<Mutex<TreeState>>> {
        Ok(self.treestate.clone())
    }

    fn get_client(&self) -> Option<Arc<dyn WorkingCopyClient>> {
        Some(self.client.clone())
    }
}
