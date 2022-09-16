/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use hgcommits::DagCommits;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use parking_lot::RwLock;
use types::HgId;

pub struct TreeManifestResolver {
    dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
    tree_store: Arc<dyn TreeStore + Send + Sync>,
}
impl TreeManifestResolver {
    pub fn new(
        dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
    ) -> Self {
        TreeManifestResolver {
            dag_commits,
            tree_store,
        }
    }
}

impl ReadTreeManifest for TreeManifestResolver {
    fn get(&self, commit_id: &HgId) -> Result<Arc<RwLock<TreeManifest>>> {
        let commit_store = self.dag_commits.read().to_dyn_read_root_tree_ids();
        let tree_ids =
            async_runtime::block_on(commit_store.read_root_tree_ids(vec![commit_id.clone()]))?;
        Ok(Arc::new(RwLock::new(TreeManifest::durable(
            self.tree_store.clone(),
            tree_ids[0].1,
        ))))
    }
}
