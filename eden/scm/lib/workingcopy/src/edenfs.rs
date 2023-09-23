/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use io::IO;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use thrift_types::edenfs::ScmFileStatus;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;
use types::RepoPathBuf;
use vfs::VFS;

use crate::filesystem::PendingChange;
use crate::filesystem::PendingChanges;

pub struct EdenFileSystem {
    root: PathBuf,
    treestate: Arc<Mutex<TreeState>>,
}

impl EdenFileSystem {
    pub fn new(vfs: VFS, treestate: Arc<Mutex<TreeState>>) -> Result<Self> {
        Ok(EdenFileSystem {
            treestate,
            root: vfs.root().to_path_buf(),
        })
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

        let result = edenfs_client::status::get_status(&self.root, p1, include_ignored)?;
        Ok(Box::new(result.status.entries.into_iter().map(
            |(path, status)| {
                {
                    // TODO: Handle non-UTF8 encoded paths from Eden
                    let repo_path = match RepoPathBuf::from_utf8(path) {
                        Ok(repo_path) => repo_path,
                        Err(err) => return Err(anyhow!(err)),
                    };

                    tracing::trace!(%repo_path, ?status, "eden status");

                    match status {
                        ScmFileStatus::REMOVED => Ok(PendingChange::Deleted(repo_path)),
                        ScmFileStatus::IGNORED => Ok(PendingChange::Ignored(repo_path)),
                        _ => Ok(PendingChange::Changed(repo_path)),
                    }
                }
            },
        )))
    }
}
