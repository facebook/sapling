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
pub use crate::store::SqlGitPushRedirectConfig;
pub use crate::store::SqlGitPushRedirectConfigBuilder;
pub use crate::types::GitPushRedirectConfigEntry;
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
pub trait GitPushRedirectConfig: Send + Sync {
    async fn set(&self, ctx: &CoreContext, repo_id: RepositoryId, mononoke: bool) -> Result<()>;

    async fn get_by_repo_id(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<GitPushRedirectConfigEntry>>;

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>>;

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>>;
}

#[derive(Clone)]
pub struct NoopGitPushRedirectConfig {}

#[async_trait]
impl GitPushRedirectConfig for NoopGitPushRedirectConfig {
    async fn set(&self, _ctx: &CoreContext, _repo_id: RepositoryId, _mononoke: bool) -> Result<()> {
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        _repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<GitPushRedirectConfigEntry>> {
        Ok(None)
    }

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        Ok(vec![])
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        Ok(vec![])
    }
}

#[derive(Clone)]
pub struct TestGitPushRedirectConfig {
    entries: Arc<Mutex<HashMap<RepositoryId, GitPushRedirectConfigEntry>>>,
}

impl TestGitPushRedirectConfig {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl GitPushRedirectConfig for TestGitPushRedirectConfig {
    async fn set(&self, _ctx: &CoreContext, repo_id: RepositoryId, mononoke: bool) -> Result<()> {
        let mut map = self.entries.lock().expect("poisoned lock");
        map.insert(
            repo_id.to_owned(),
            GitPushRedirectConfigEntry {
                id: RowId(0),
                repo_id,
                mononoke,
            },
        );
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<GitPushRedirectConfigEntry>> {
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
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| entry.mononoke)
            .cloned()
            .collect())
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| !entry.mononoke)
            .cloned()
            .collect())
    }
}
