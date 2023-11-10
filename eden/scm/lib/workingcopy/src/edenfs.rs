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
use std::time::SystemTime;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use edenfs_client::EdenFsClient;
use edenfs_client::FileStatus;
use io::IO;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use parking_lot::RwLock;
use pathmatcher::DynMatcher;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;

use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;

pub struct EdenFileSystem {
    treestate: Arc<Mutex<TreeState>>,
    client: EdenFsClient,

    // For wait_for_potential_change
    journal_position: Cell<(i64, i64)>,
}

impl EdenFileSystem {
    pub fn new(treestate: Arc<Mutex<TreeState>>, client: EdenFsClient) -> Result<Self> {
        let journal_position = Cell::new(client.get_journal_position()?);
        Ok(EdenFileSystem {
            treestate,
            client,
            journal_position,
        })
    }
}

impl FileSystem for EdenFileSystem {
    fn pending_changes(
        &self,
        _matcher: DynMatcher,
        _ignore_matcher: DynMatcher,
        _ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
        _last_write: SystemTime,
        _config: &dyn Config,
        _io: &IO,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let p1 = self
            .treestate
            .lock()
            .parents()
            .next()
            .unwrap_or_else(|| Ok(NULL_ID))?;

        let status_map = self.client.get_status(p1, include_ignored)?;
        Ok(Box::new(status_map.into_iter().map(|(path, status)| {
            tracing::trace!(%path, ?status, "eden status");

            match status {
                FileStatus::Removed => Ok(PendingChange::Deleted(path)),
                FileStatus::Ignored => Ok(PendingChange::Ignored(path)),
                _ => Ok(PendingChange::Changed(path)),
            }
        })))
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
        manifests: &[Arc<RwLock<TreeManifest>>],
        _dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        assert!(!manifests.is_empty());
        Ok(None)
    }
}
