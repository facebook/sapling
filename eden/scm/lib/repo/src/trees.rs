/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use commits_trait::DagCommits;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use parking_lot::RwLock;
use types::hgid;
use types::HgId;

pub struct TreeManifestResolver {
    dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
    tree_store: Arc<dyn TreeStore>,
}

impl TreeManifestResolver {
    pub fn new(
        dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
        tree_store: Arc<dyn TreeStore>,
    ) -> Self {
        TreeManifestResolver {
            dag_commits,
            tree_store,
        }
    }
}

impl ReadTreeManifest for TreeManifestResolver {
    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        if commit_id.is_null() {
            // Null commit represents a working copy with no parents. Avoid
            // querying the backend since this is not a real commit.
            return Ok(TreeManifest::ephemeral(self.tree_store.clone()));
        }

        Ok(TreeManifest::durable(
            self.tree_store.clone(),
            self.get_root_id(commit_id)?,
        ))
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        if commit_id.is_null() {
            // Special case: null commit's manifest node is null.
            return Ok(hgid::NULL_ID);
        }

        let commit_store = self.dag_commits.read().to_dyn_read_root_tree_ids();
        let tree_ids =
            async_runtime::block_on(commit_store.read_root_tree_ids(vec![commit_id.clone()]))?;

        if tree_ids.is_empty() {
            bail!("no root tree id for commit {commit_id}");
        }

        Ok(tree_ids[0].1)
    }
}
