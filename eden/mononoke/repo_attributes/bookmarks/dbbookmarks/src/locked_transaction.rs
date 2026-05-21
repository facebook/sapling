/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A transaction type that acquires a per-bookmark FOR UPDATE lock at
//! construction time and holds it for the lifetime of the transaction.
//!
//! This is used by pessimistic pushrebase: the lock is acquired BEFORE
//! the rebase so that only one writer per bookmark does work at a time.
//! CAS is retained as defense-in-depth (the UPDATE uses WHERE
//! changeset_id = old_id even though the lock guarantees exclusivity).

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_ext::Connection;
use sql_ext::Transaction as SqlTransaction;

use crate::transaction::AddBookmarkLog;
use crate::transaction::ReadBookmarkUnderLock;
use crate::transaction::UpdateBookmark;
use crate::transaction::acquire_single_bookmark_lock;
use crate::transaction::allocate_log_ids_from_sequence;

/// A bookmark transaction that holds a SQL-level per-bookmark lock.
///
/// Created via `SqlBookmarks::start_locked_transaction`. The lock is
/// acquired during construction (via `SELECT ... FOR UPDATE`) and held
/// until `commit()` or `rollback()` is called.
///
/// The current bookmark value is read under the lock and cached. Callers
/// use `current_value()` to get it, perform the rebase, then call
/// `commit()` to finalize the bookmark move within the same transaction.
pub struct LockedBookmarkTransaction {
    /// The open SQL transaction holding the FOR UPDATE lock.
    /// Wrapped in Option so we can take it in commit/rollback.
    txn: Option<SqlTransaction>,

    /// The repository this transaction applies to.
    repo_id: RepositoryId,

    /// The bookmark being locked.
    bookmark: BookmarkKey,

    /// The bookmark value read under the lock (None if the bookmark
    /// does not exist yet).
    current_value: Option<ChangesetId>,
}

impl LockedBookmarkTransaction {
    /// Start a new locked transaction: opens a SQL transaction, acquires the
    /// per-bookmark FOR UPDATE lock, and reads the current bookmark value.
    pub(crate) async fn new(
        ctx: &CoreContext,
        write_connection: &Connection,
        repo_id: RepositoryId,
        bookmark: BookmarkKey,
    ) -> Result<Self> {
        let mut txn = write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await
            .context("Failed to start SQL transaction for locked bookmark")?;

        // Acquire the per-bookmark row lock.
        txn = acquire_single_bookmark_lock(txn, &repo_id, bookmark.name()).await?;

        // Read the current bookmark value under the lock.
        let (txn_, rows) = ReadBookmarkUnderLock::query_with_transaction(
            txn,
            &repo_id,
            bookmark.name(),
            bookmark.category(),
        )
        .await?;
        txn = txn_;

        let current_value = rows.into_iter().next().map(|(cs_id,)| cs_id);

        Ok(Self {
            txn: Some(txn),
            repo_id,
            bookmark,
            current_value,
        })
    }

    /// The bookmark value as read under the lock. Returns `None` if the
    /// bookmark does not exist.
    pub fn current_value(&self) -> Option<ChangesetId> {
        self.current_value
    }

    /// The bookmark key this transaction locks.
    pub fn bookmark(&self) -> &BookmarkKey {
        &self.bookmark
    }

    /// Commit a bookmark update within this locked transaction.
    ///
    /// Performs a CAS UPDATE (defense-in-depth), inserts into the bookmark
    /// update log, runs transaction hooks, and commits.
    ///
    /// Returns `Ok(Some(log_id))` on success, `Ok(None)` if the CAS check
    /// failed (should not happen under the lock, but retained for safety),
    /// or an error on infrastructure failure.
    pub async fn commit(
        mut self,
        ctx: &CoreContext,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> Result<Option<u64>> {
        let mut txn = self
            .txn
            .take()
            .ok_or_else(|| anyhow!("LockedBookmarkTransaction already consumed"))?;

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let old_cs = self
            .current_value
            .ok_or_else(|| anyhow!("Cannot update a bookmark that does not exist"))?;

        // Allocate a log ID via auto-increment (with per-repo seed check).
        let (txn_, ids) = allocate_log_ids_from_sequence(txn, self.repo_id, 1).await?;
        txn = txn_;
        let log_id = ids
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("allocate_log_ids_from_sequence returned empty"))?;

        // CAS UPDATE (defense-in-depth: the lock guarantees exclusivity,
        // but we still verify old_id matches).
        let (txn_, result) = UpdateBookmark::query_with_transaction(
            txn,
            &self.repo_id,
            &Some(log_id),
            self.bookmark.name(),
            self.bookmark.category(),
            &old_cs,
            &new_cs,
            BookmarkKind::ALL_PUBLISHING,
        )
        .await?;
        txn = txn_;

        if result.affected_rows() != 1 {
            // CAS failed. This should not happen under the lock, but
            // return None rather than error to match the existing
            // BookmarkTransaction::commit contract.
            txn.rollback().await?;
            return Ok(None);
        }

        // Insert into bookmarks_update_log.
        let timestamp = Timestamp::now();
        let data = [(
            &log_id,
            &self.repo_id,
            self.bookmark.name(),
            self.bookmark.category(),
            &Some(old_cs),
            &Some(new_cs),
            &reason,
            &timestamp,
        )];
        let (txn_, _) = AddBookmarkLog::query_with_transaction(txn, &data[..]).await?;
        txn = txn_;

        // Run transaction hooks.
        for hook in &txn_hooks {
            txn = hook(ctx.clone(), txn)
                .await
                .map_err(|e| match e {
                    BookmarkTransactionError::RetryableError(e) => e,
                    BookmarkTransactionError::LogicError => {
                        anyhow!("Transaction hook returned LogicError")
                    }
                    BookmarkTransactionError::Other(e) => e,
                })
                .context("Transaction hook failed in LockedBookmarkTransaction::commit")?;
        }

        // Commit the SQL transaction (releases the lock).
        txn.commit().await?;

        Ok(Some(log_id))
    }

    /// Roll back the transaction, releasing the lock without making
    /// any changes.
    pub async fn rollback(mut self) -> Result<()> {
        if let Some(txn) = self.txn.take() {
            txn.rollback().await?;
        }
        Ok(())
    }
}

impl Drop for LockedBookmarkTransaction {
    fn drop(&mut self) {
        if self.txn.is_some() {
            tracing::warn!(
                "LockedBookmarkTransaction for {:?} dropped without commit/rollback; \
                 SQL transaction will be implicitly rolled back",
                self.bookmark,
            );
        }
    }
}
