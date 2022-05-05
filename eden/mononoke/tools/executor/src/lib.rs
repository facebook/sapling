/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::*;
#[cfg(not(fbcode_build))]
pub use oss::*;

// BP: Background Process, e.g. Walker.
// BPE: BackgroundProcessExecutor e.g. Executor wrapper over walker for
// sharded execution.

/// Trait outlining the methods to be implemented by Mononoke jobs that require
/// sharding across different repos based on their resource requirements.
#[async_trait]
pub trait RepoShardedJob: Send + Sync {
    /// Callback for when the BPE for the current job gets a repo to execute.
    async fn on_repo_load(&self, maybe_repo_name: Option<&str>) -> Result<()>;

    /// Callback for when the BPE for the current job is required to relinquish
    /// an existing repo assigned to it earlier. The timeout specified during
    /// creation determines the time for which the BPE will wait before
    /// performing a hard-cleaup of the repo associated task.
    async fn on_repo_unload(&self, maybe_repo_name: Option<&str>) -> Result<()>;
}
