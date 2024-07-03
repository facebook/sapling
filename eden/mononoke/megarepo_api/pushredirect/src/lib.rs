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

pub use crate::types::PushRedirectionEntry;
pub use crate::types::RowId;

#[facet::facet]
#[async_trait]
pub trait PushRedirection: Send + Sync {
    async fn set(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        draft_push: bool,
        public_push: bool,
    ) -> Result<RowId>;

    /// Get the full entry by id
    /// It is mainly intended to be used in tests.
    async fn test_get_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<PushRedirectionEntry>>;

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
    ) -> Result<Option<PushRedirectionEntry>>;
}
