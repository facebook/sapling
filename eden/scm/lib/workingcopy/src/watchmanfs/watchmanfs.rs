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
use manifest_tree::ReadTreeManifest;
use parking_lot::Mutex;
use pathmatcher::Matcher;
use treestate::treestate::TreeState;
use vfs::VFS;
use watchman_client::prelude::*;

use super::state::StatusQuery;
use super::state::WatchmanState;
use super::treestate::WatchmanTreeState;
use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;
use crate::workingcopy::WorkingCopy;

type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct WatchmanFileSystem {
    vfs: VFS,
    treestate: Arc<Mutex<TreeState>>,
    tree_resolver: ArcReadTreeManifest,
    store: ArcReadFileContents,
}

impl WatchmanFileSystem {
    pub fn new(
        root: PathBuf,
        treestate: Arc<Mutex<TreeState>>,
        tree_resolver: ArcReadTreeManifest,
        store: ArcReadFileContents,
    ) -> Result<Self> {
        Ok(WatchmanFileSystem {
            vfs: VFS::new(root)?,
            treestate,
            tree_resolver,
            store,
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
        last_write: SystemTime,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let state = WatchmanState::new(WatchmanTreeState {
            treestate: self.treestate.clone(),
        })?;

        let result = async_runtime::block_on(self.query_result(&state))?;

        let manifests =
            WorkingCopy::current_manifests(&self.treestate.lock(), &self.tree_resolver)?;

        let file_change_detector = FileChangeDetector::new(
            self.treestate.clone(),
            self.vfs.clone(),
            last_write.try_into()?,
            manifests[0].clone(),
            self.store.clone(),
        );
        let mut pending_changes = state.merge(result, file_change_detector)?;

        pending_changes.persist(WatchmanTreeState {
            treestate: self.treestate.clone(),
        })?;

        Ok(Box::new(pending_changes.into_iter()))
    }
}
