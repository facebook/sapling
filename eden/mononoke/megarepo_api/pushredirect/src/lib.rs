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

pub use crate::store::SqlPushRedirectionConfig;
pub use crate::store::SqlPushRedirectionConfigBuilder;
pub use crate::types::PushRedirectionConfigEntry;
pub use crate::types::RowId;

#[facet::facet]
#[async_trait]
pub trait PushRedirectionConfig: Send + Sync {
    async fn set(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        draft_push: bool,
        public_push: bool,
    ) -> Result<()>;

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<PushRedirectionConfigEntry>>;
}

#[derive(Clone)]
pub struct NoopPushRedirectionConfig {}

#[async_trait]
impl PushRedirectionConfig for NoopPushRedirectionConfig {
    async fn set(
        &self,
        _ctx: &CoreContext,
        _repo_id: RepositoryId,
        _draft_push: bool,
        _public_push: bool,
    ) -> Result<()> {
        Ok(())
    }

    async fn get(
        &self,
        _ctx: &CoreContext,
        _repo_id: RepositoryId,
    ) -> Result<Option<PushRedirectionConfigEntry>> {
        Ok(None)
    }
}

#[derive(Clone)]
pub struct TestPushRedirectionConfig {
    entries: Arc<Mutex<HashMap<RepositoryId, PushRedirectionConfigEntry>>>,
}

impl TestPushRedirectionConfig {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl PushRedirectionConfig for TestPushRedirectionConfig {
    async fn set(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        draft_push: bool,
        public_push: bool,
    ) -> Result<()> {
        let mut map = self.entries.lock().expect("poisoned lock");
        map.insert(
            repo_id.to_owned(),
            PushRedirectionConfigEntry {
                id: RowId(0),
                repo_id,
                draft_push,
                public_push,
            },
        );
        Ok(())
    }

    async fn get(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<PushRedirectionConfigEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .cloned())
    }
}
