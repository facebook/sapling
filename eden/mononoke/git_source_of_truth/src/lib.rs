/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;

mod store;
mod types;
pub use crate::store::SqlGitSourceOfTruthConfig;
pub use crate::store::SqlGitSourceOfTruthConfigBuilder;
pub use crate::types::GitSourceOfTruth;
pub use crate::types::GitSourceOfTruthConfigEntry;
pub use crate::types::RepositoryName;
pub use crate::types::RowId;

/// Enum representing the staleness of the SoT status for a repo
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Staleness {
    /// The most recent state of the SoT flag for the given repo
    MostRecent,
    /// SoT state for the given repo with best-effort recency
    MaybeStale,
}

#[facet::facet]
#[async_trait]
pub trait GitSourceOfTruthConfig: Send + Sync {
    async fn set(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        repo_name: RepositoryName,
        source_of_truth: GitSourceOfTruth,
    ) -> Result<()>;

    async fn get_by_repo_id(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<GitSourceOfTruthConfigEntry>>;

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>>;

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>>;

    async fn get_locked(&self, _ctx: &CoreContext) -> Result<Vec<GitSourceOfTruthConfigEntry>>;
}

#[derive(Clone)]
pub struct NoopGitSourceOfTruthConfig {}

#[async_trait]
impl GitSourceOfTruthConfig for NoopGitSourceOfTruthConfig {
    async fn set(
        &self,
        _ctx: &CoreContext,
        _repo_id: RepositoryId,
        _repo_name: RepositoryName,
        _source_of_truth: GitSourceOfTruth,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        _repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<GitSourceOfTruthConfigEntry>> {
        Ok(None)
    }

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(vec![])
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(vec![])
    }

    async fn get_locked(&self, _ctx: &CoreContext) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(vec![])
    }
}

#[derive(Clone)]
pub struct TestGitSourceOfTruthConfig {
    entries: Arc<Mutex<HashMap<RepositoryId, GitSourceOfTruthConfigEntry>>>,
}

impl TestGitSourceOfTruthConfig {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl GitSourceOfTruthConfig for TestGitSourceOfTruthConfig {
    async fn set(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        repo_name: RepositoryName,
        source_of_truth: GitSourceOfTruth,
    ) -> Result<()> {
        let mut map = self.entries.lock().expect("poisoned lock");
        map.insert(
            repo_id.to_owned(),
            GitSourceOfTruthConfigEntry {
                id: RowId(0),
                repo_id,
                repo_name,
                source_of_truth,
            },
        );
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<GitSourceOfTruthConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .cloned())
    }

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| entry.source_of_truth == GitSourceOfTruth::Mononoke)
            .cloned()
            .collect())
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| entry.source_of_truth == GitSourceOfTruth::Metagit)
            .cloned()
            .collect())
    }

    async fn get_locked(&self, _ctx: &CoreContext) -> Result<Vec<GitSourceOfTruthConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| entry.source_of_truth == GitSourceOfTruth::Locked)
            .cloned()
            .collect())
    }
}
