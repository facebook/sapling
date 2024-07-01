/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use megarepo_configs::Source;
use megarepo_configs::SyncConfigVersion;
use mononoke_types::RepositoryId;

pub mod store;
mod types;

pub use crate::db::store::SqlMegarepoSyncConfig;
pub use crate::db::types::MegarepoSyncConfigEntry;
pub use crate::db::types::RowId;

/// A store of Megarepo Sync Configs
#[facet::facet]
#[async_trait]
pub trait MegarepoSyncConfig: Send + Sync {
    async fn add_repo_config(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        version: &SyncConfigVersion,
        sources: Vec<Source>,
    ) -> Result<RowId>;

    /// Get the full request object entry by id
    /// Mainly intended to be used in tests.
    #[allow(dead_code)]
    async fn test_get_repo_config_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<MegarepoSyncConfigEntry>>;

    async fn get_repo_config_by_version(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        version: &SyncConfigVersion,
    ) -> Result<Option<MegarepoSyncConfigEntry>>;
}
