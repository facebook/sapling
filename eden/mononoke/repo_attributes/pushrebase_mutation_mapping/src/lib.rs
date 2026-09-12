/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod save_mapping_pushrebase_hook;
mod sql_queries;
#[cfg(test)]
mod test;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkTransactionHook;
use context::CoreContext;
use futures::future::FutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use pushrebase_hook::PushrebaseHook;
pub use sql_queries::SqlPushrebaseMutationMapping;
pub use sql_queries::SqlPushrebaseMutationMappingConnection;
pub use sql_queries::add_pushrebase_mapping;
pub use sql_queries::get_prepushrebase_ids;
pub use sql_queries::get_successor_ids;

/// `add_pushrebase_mapping` as a hook for a writer that runs its own bookmark
/// transaction instead of pushrebase's.
///
/// Re-running is safe because each attempt opens a fresh transaction and a
/// failed one is rolled back, so a retry never sees the previous attempt's
/// rows. The table has no unique key, so a replay outside the rollback
/// envelope would genuinely duplicate rows, and duplicates are not harmless:
/// `commit_lookup_pushrebase_history` treats two predecessors for one
/// successor as an ambiguity error. Keep the write inside the transaction.
///
/// A SQL fault is retryable, unlike `SaveMappingPushrebaseHook` where it is
/// fatal: a caller with its own retry loop should not fail a whole land on a
/// transient fault on the bookmarks master. The cost is that a permanent
/// fault burns that caller's retry budget before surfacing.
pub fn add_pushrebase_mapping_hook(
    entries: Vec<PushrebaseMutationMappingEntry>,
) -> BookmarkTransactionHook {
    let entries = Arc::new(entries);
    Arc::new(move |_ctx, txn| {
        let entries = entries.clone();
        async move {
            add_pushrebase_mapping(txn, &entries)
                .await
                .map_err(BookmarkTransactionError::RetryableError)
        }
        .boxed()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushrebaseMutationMappingEntry {
    repo_id: RepositoryId,
    predecessor_bcs_id: ChangesetId,
    successor_bcs_id: ChangesetId,
}

impl PushrebaseMutationMappingEntry {
    pub fn new(
        repo_id: RepositoryId,
        predecessor_bcs_id: ChangesetId,
        successor_bcs_id: ChangesetId,
    ) -> Self {
        Self {
            repo_id,
            predecessor_bcs_id,
            successor_bcs_id,
        }
    }
}

#[async_trait]
#[facet::facet]
pub trait PushrebaseMutationMapping: Send + Sync {
    fn get_hook(&self) -> Option<Box<dyn PushrebaseHook>>;
    async fn get_prepushrebase_ids(
        &self,
        ctx: &CoreContext,
        successor_bcs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>>;

    /// The commits pushrebase rewrote `predecessor_bcs_id` into.
    ///
    /// Several are possible: the table has no unique constraint, and the same
    /// commit can be landed more than once. Callers must handle an ambiguous
    /// answer rather than assuming one successor.
    async fn get_successor_ids(
        &self,
        ctx: &CoreContext,
        predecessor_bcs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>>;
}
