/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
