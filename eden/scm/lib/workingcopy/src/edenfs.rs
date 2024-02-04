/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use edenfs_client::EdenFsClient;
use edenfs_client::FileStatus;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use storemodel::FileStore;
use termlogger::TermLogger;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;
use types::HgId;
use vfs::VFS;

use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;

pub struct EdenFileSystem {
    treestate: Arc<Mutex<TreeState>>,
    client: Arc<EdenFsClient>,
    vfs: VFS,
    store: Arc<dyn FileStore>,

    // For wait_for_potential_change
    journal_position: Cell<(i64, i64)>,
}

impl EdenFileSystem {
    pub fn new(
        treestate: Arc<Mutex<TreeState>>,
        client: Arc<EdenFsClient>,
        vfs: VFS,
        store: Arc<dyn FileStore>,
    ) -> Result<Self> {
        let journal_position = Cell::new(client.get_journal_position()?);
        Ok(EdenFileSystem {
            treestate,
            client,
            vfs,
            store,
            journal_position,
        })
    }
}

impl FileSystem for EdenFileSystem {
    fn pending_changes(
        &self,
        matcher: DynMatcher,
        _ignore_matcher: DynMatcher,
        _ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
        _config: &dyn Config,
        _lgr: &TermLogger,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let p1 = self
            .treestate
            .lock()
            .parents()
            .next()
            .unwrap_or_else(|| Ok(NULL_ID))?;

        let status_map = self.client.get_status(p1, include_ignored)?;
        Ok(Box::new(status_map.into_iter().filter_map(
            move |(path, status)| {
                tracing::trace!(%path, ?status, "eden status");

                // EdenFS reports files that are present in the overlay but filtered from the repo
                // as untracked. We "drop" any files that are excluded by the current filter.
                match matcher.matches_file(&path) {
                    Ok(m) if m => {
                        Some(match status {
                            FileStatus::Removed => Ok(PendingChange::Deleted(path)),
                            FileStatus::Ignored => Ok(PendingChange::Ignored(path)),
                            _ => Ok(PendingChange::Changed(path)),
                        })
                    },
                    Ok(_) => {
                        None
                    },
                    Err(e) => {
                        tracing::warn!(
                            "failed to determine if {} is ignored or not tracked by the active filter: {:?}",
                            &path,
                            e
                        );
                        Some(Err(e))
                    }
                }
            },
        )))
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
}
