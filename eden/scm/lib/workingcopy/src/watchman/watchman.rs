/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use treestate::treestate::TreeState;
use vfs::VFS;
use watchman_client::prelude::*;

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

    pub async fn pending_changes(
        &self,
        treestate: Arc<Mutex<TreeState>>,
    ) -> Result<impl Iterator<Item = Result<PendingChangeResult>>> {
        let client = Connector::new().connect().await?;
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let mut state = WatchmanState::new(WatchmanTreeState {
            treestate: treestate.lock(),
        });

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: state.get_clock(),
                    ..Default::default()
                },
            )
            .await?;
        state.merge(result);

        state.persist(WatchmanTreeState {
            treestate: treestate.lock(),
        })?;

        Ok(state.pending_changes())
    }
}
