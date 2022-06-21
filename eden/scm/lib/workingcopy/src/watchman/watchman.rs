/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use parking_lot::RwLock;
use treestate::treestate::TreeState;
use vfs::VFS;
use watchman_client::prelude::*;

use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::HgModifiedTime;
use crate::filesystem::PendingChangeResult;

use super::state::StatusQuery;
use super::state::WatchmanState;
use super::treestate::WatchmanTreeState;

pub struct Watchman {
    vfs: VFS,
}

impl Watchman {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Watchman {
            vfs: VFS::new(root)?,
        })
    }

    pub fn pending_changes(
        &self,
        treestate: Arc<Mutex<TreeState>>,
        last_write: HgModifiedTime,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
    ) -> Result<impl Iterator<Item = Result<PendingChangeResult>>> {
        let file_change_detector = FileChangeDetector::new(
            treestate.clone(),
            self.vfs.clone(),
            last_write,
            manifest,
            store,
        );
        let state = WatchmanState::new(
            WatchmanTreeState {
                treestate: treestate.lock(),
            },
            file_change_detector,
        )?;
        let result = async_runtime::block_on(self.query_result(&state))?;

        let treestate = WatchmanTreeState {
            treestate: treestate.lock(),
        };

        let pending_changes = state.merge(result, treestate);
        pending_changes.map(|result| result.into_iter())
    }

    async fn query_result(&self, state: &WatchmanState) -> Result<QueryResult<StatusQuery>> {
        let client = Connector::new().connect().await?;
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: state.get_clock(),
                    ..Default::default()
                },
            )
            .await?;

        Ok(result)
    }
}
