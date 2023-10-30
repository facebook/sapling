/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use configmodel::Config;
use edenfs_client::EdenFsClient;
use edenfs_client::FileStatus;
use io::IO;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;

use crate::filesystem::PendingChange;
use crate::filesystem::PendingChanges;

pub struct EdenFileSystem {
    treestate: Arc<Mutex<TreeState>>,
    client: EdenFsClient,
}

impl EdenFileSystem {
    pub fn new(treestate: Arc<Mutex<TreeState>>, client: EdenFsClient) -> Result<Self> {
        Ok(EdenFileSystem { treestate, client })
    }
}

impl PendingChanges for EdenFileSystem {
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
}
