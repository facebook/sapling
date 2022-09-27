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
use pathmatcher::Matcher;
use treestate::treestate::TreeState;
use vfs::VFS;
use watchman_client::prelude::*;

use super::state::StatusQuery;
use super::state::WatchmanState;
use super::treestate::WatchmanTreeState;
use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::HgModifiedTime;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;

pub struct WatchmanFileSystem {
    vfs: VFS,
    treestate: Arc<Mutex<TreeState>>,
    manifest: Arc<RwLock<TreeManifest>>,
    store: ArcReadFileContents,
    last_write: HgModifiedTime,
}

impl WatchmanFileSystem {
    pub fn new(
        root: PathBuf,
        treestate: Arc<Mutex<TreeState>>,
        manifest: Arc<RwLock<TreeManifest>>,
        store: ArcReadFileContents,
        last_write: HgModifiedTime,
    ) -> Result<Self> {
        Ok(WatchmanFileSystem {
            vfs: VFS::new(root)?,
            treestate,
            manifest,
            store,
            last_write,
        })
    }

    async fn query_result(&self, state: &WatchmanState) -> Result<QueryResult<StatusQuery>> {
        let client = Connector::new().connect().await?;
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let ident = identity::must_sniff_dir(self.vfs.root())?;
        let excludes = Expr::Any(vec![Expr::DirName(DirNameTerm {
            path: PathBuf::from(ident.dot_dir()),
            depth: None,
        })]);

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: state.get_clock(),
                    expression: Some(Expr::Not(Box::new(excludes))),
                    ..Default::default()
                },
            )
            .await?;

        Ok(result)
    }
}

impl PendingChanges for WatchmanFileSystem {
    fn pending_changes(
        &self,
        _matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let state = WatchmanState::new(WatchmanTreeState {
            treestate: self.treestate.clone(),
        })?;

        let result = async_runtime::block_on(self.query_result(&state))?;

        let file_change_detector = FileChangeDetector::new(
            self.treestate.clone(),
            self.vfs.clone(),
            self.last_write.clone(),
            self.manifest.clone(),
            self.store.clone(),
        );
        let mut pending_changes = state.merge(result, file_change_detector)?;

        pending_changes.persist(WatchmanTreeState {
            treestate: self.treestate.clone(),
        })?;

        Ok(Box::new(pending_changes.into_iter()))
    }
}
