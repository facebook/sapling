/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `ConfigUpdateReceiver` — the callback interface registered with
//! `MononokeConfigs` for receiving config update notifications.
//!
//! Two delivery paths:
//! - `apply_update` fires on bulk reloads (legacy blob or tier manifest change)
//!   and hands the receiver the entire `RepoConfigs` Arc.
//! - `apply_repo_update` fires on per-repo content changes in the split-loading
//!   path and hands the receiver just the one repo's new `RepoConfig`. Default
//!   no-op implementation lets receivers that only care about bulk updates
//!   skip implementing it.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::RepoConfig;

/// Trait defining methods related to config update notification. A struct
/// implementing this trait can be configured to receive the most updated
/// config value every time the underlying config changes.
#[async_trait]
pub trait ConfigUpdateReceiver: Send + Sync {
    /// Called on bulk reloads (legacy blob or tier-manifest change). The
    /// receiver is handed the entire new `RepoConfigs` and `StorageConfigs`.
    ///
    /// This should not be too long running since config updates wait for
    /// all receivers to complete before checking for the next config update.
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        storage_configs: Arc<StorageConfigs>,
    ) -> Result<()>;

    /// Called when a single repo's config changes in the per-repo split-loading
    /// path (i.e. an edit to a `repos/git/<hash>/<name>.cconf` file that does
    /// not modify the tier manifest's membership).
    ///
    /// The bulk `RepoConfigs` Arc accessible via `MononokeConfigs::repo_configs`
    /// has already been patched with `repo_config` by the watcher loop, so any
    /// receiver code that reads the bulk Arc inside this method sees the new
    /// state for `repo_name`. See `apply_per_repo_update` in `watcher.rs` for
    /// the ordering invariant.
    ///
    /// Default implementation is a no-op — existing receivers that only handle
    /// bulk updates don't need to implement it.
    async fn apply_repo_update(&self, _repo_name: &str, _repo_config: &RepoConfig) -> Result<()> {
        Ok(())
    }
}
