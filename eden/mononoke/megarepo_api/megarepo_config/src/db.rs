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
    /// Insert a config for `(repo_id, bookmark, version)`.
    ///
    /// Inserts are idempotent: re-inserting byte-identical content returns
    /// the existing row instead of creating a duplicate. Re-inserting
    /// *different* content for an existing key is rejected once the
    /// `scm/mononoke:megarepo_reject_divergent_config_reinsert` JustKnob is enabled
    /// (before that it succeeds but keeps the pre-existing row).
    async fn add_repo_config(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        version: &SyncConfigVersion,
        sources: Vec<Source>,
    ) -> Result<RowId>;

    /// Get the full request object entry by id
    #[cfg(test)]
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
