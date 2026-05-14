/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use dbbookmarks::transaction::AcquireBookmarkLock;
use dbbookmarks::transaction::AddBookmarkLog;
use dbbookmarks::transaction::AllocateBookmarkLogId;
use dbbookmarks::transaction::DeleteBookmarkIf;
use dbbookmarks::transaction::EnsureBookmarkLockRow;
use dbbookmarks::transaction::FindGlobalMaxBookmarkLogId;
use dbbookmarks::transaction::FindMaxBookmarkLogId;
use dbbookmarks::transaction::InsertBookmarks;
use dbbookmarks::transaction::ReadLastInsertId;
use dbbookmarks::transaction::ReadMaxSequenceId;
use dbbookmarks::transaction::SeedSequenceId;
use dbbookmarks::transaction::UpdateBookmark;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_ext::Connection;
use sql_ext::Transaction as SqlTransaction;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.multi_repo_land.commit";
    success: timeseries(Rate, Sum),
    cas_failure: timeseries(Rate, Sum),
    retry: timeseries(Rate, Sum),
    retryable_error_exhausted: timeseries(Rate, Sum),
    other_error: timeseries(Rate, Sum),
    attempt_count: timeseries(Rate, Average, Sum),
}

/// A bookmark operation to be executed atomically in a multi-repo transaction.
enum BookmarkOp {
    Update {
        repo_id: RepositoryId,
        bookmark: BookmarkKey,
        old_cs_id: ChangesetId,
        new_cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    },
    Create {
        repo_id: RepositoryId,
        bookmark: BookmarkKey,
        cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    },
    Delete {
        repo_id: RepositoryId,
        bookmark: BookmarkKey,
        old_cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    },
}

impl BookmarkOp {
    fn repo_id(&self) -> RepositoryId {
        match self {
            Self::Update { repo_id, .. }
            | Self::Create { repo_id, .. }
            | Self::Delete { repo_id, .. } => *repo_id,
        }
    }

    fn bookmark(&self) -> &BookmarkKey {
        match self {
            Self::Update { bookmark, .. }
            | Self::Create { bookmark, .. }
            | Self::Delete { bookmark, .. } => bookmark,
        }
    }

    /// Execute this operation within a SQL transaction.
    ///
    /// Pushes a log entry and runs the appropriate SQL query.
    /// Returns `LogicError` if the CAS check fails.
    async fn execute(
        &self,
        txn: SqlTransaction,
        log: &mut TransactionLog,
    ) -> Result<SqlTransaction, BookmarkTransactionError> {
        match self {
            Self::Update {
                repo_id,
                bookmark,
                old_cs_id,
                new_cs_id,
                reason,
            } => {
                let log_id = log
                    .push(
                        *repo_id,
                        bookmark,
                        Some(*old_cs_id),
                        Some(*new_cs_id),
                        *reason,
                    )
                    .map_err(BookmarkTransactionError::RetryableError)?;
                let (txn, result) = UpdateBookmark::query_with_transaction(
                    txn,
                    repo_id,
                    &Some(log_id),
                    bookmark.name(),
                    bookmark.category(),
                    old_cs_id,
                    new_cs_id,
                    BookmarkKind::ALL_PUBLISHING,
                )
                .await?;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
                Ok(txn)
            }
            Self::Create {
                repo_id,
                bookmark,
                cs_id,
                reason,
            } => {
                let log_id = log
                    .push(*repo_id, bookmark, None, Some(*cs_id), *reason)
                    .map_err(BookmarkTransactionError::RetryableError)?;
                let data = [(
                    repo_id,
                    &Some(log_id),
                    bookmark.name(),
                    bookmark.category(),
                    cs_id,
                    &BookmarkKind::PullDefaultPublishing,
                )];
                let (txn, result) = InsertBookmarks::query_with_transaction(txn, &data[..]).await?;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
                Ok(txn)
            }
            Self::Delete {
                repo_id,
                bookmark,
                old_cs_id,
                reason,
            } => {
                log.push(*repo_id, bookmark, Some(*old_cs_id), None, *reason)
                    .map_err(BookmarkTransactionError::RetryableError)?;
                let (txn, result) = DeleteBookmarkIf::query_with_transaction(
                    txn,
                    repo_id,
                    bookmark.name(),
                    bookmark.category(),
                    old_cs_id,
                )
                .await?;
                if result.affected_rows() != 1 {
                    return Err(BookmarkTransactionError::LogicError);
                }
                Ok(txn)
            }
        }
    }
}

/// Accumulates log entries and assigns IDs either sequentially per-repo
/// (old path) or from pre-allocated auto-increment IDs (new path).
struct TransactionLog {
    next_log_ids: HashMap<RepositoryId, u64>,
    entries: Vec<LogEntry>,
    pre_allocated_ids: Option<Vec<u64>>,
    pre_allocated_cursor: usize,
}

struct LogEntry {
    id: u64,
    repo_id: RepositoryId,
    bookmark: BookmarkKey,
    old: Option<ChangesetId>,
    new: Option<ChangesetId>,
    reason: BookmarkUpdateReason,
}

impl TransactionLog {
    fn new(next_log_ids: HashMap<RepositoryId, u64>) -> Self {
        Self {
            next_log_ids,
            entries: Vec::new(),
            pre_allocated_ids: None,
            pre_allocated_cursor: 0,
        }
    }

    fn from_pre_allocated(ids: Vec<u64>) -> Self {
        Self {
            next_log_ids: HashMap::new(),
            entries: Vec::new(),
            pre_allocated_ids: Some(ids),
            pre_allocated_cursor: 0,
        }
    }

    fn push(
        &mut self,
        repo_id: RepositoryId,
        bookmark: &BookmarkKey,
        old: Option<ChangesetId>,
        new: Option<ChangesetId>,
        reason: BookmarkUpdateReason,
    ) -> Result<u64> {
        let id = if let Some(ref ids) = self.pre_allocated_ids {
            // Safety: pre_allocated_ids contains exactly N IDs where N is
            // the number of ops, and push() is called exactly once per op.
            // Use .get() to surface a clear error if this invariant breaks.
            let id = *ids.get(self.pre_allocated_cursor).ok_or_else(|| {
                anyhow!(
                    "Pre-allocated ID cursor {} exceeds available IDs ({})",
                    self.pre_allocated_cursor,
                    ids.len()
                )
            })?;
            self.pre_allocated_cursor += 1;
            id
        } else {
            let next_id = self.next_log_ids.entry(repo_id).or_insert(1);
            let id = *next_id;
            *next_id += 1;
            id
        };

        self.entries.push(LogEntry {
            id,
            repo_id,
            bookmark: bookmark.clone(),
            old,
            new,
            reason,
        });
        Ok(id)
    }

    /// Write all accumulated log entries into the SQL transaction.
    async fn write(self, mut txn: SqlTransaction) -> Result<SqlTransaction> {
        let timestamp = Timestamp::now();
        for entry in &self.entries {
            let data = [(
                &entry.id,
                &entry.repo_id,
                entry.bookmark.name(),
                entry.bookmark.category(),
                &entry.old,
                &entry.new,
                &entry.reason,
                &timestamp,
            )];
            txn = AddBookmarkLog::query_with_transaction(txn, &data[..])
                .await?
                .0;
        }
        Ok(txn)
    }
}

/// Result of a multi-repo bookmark transaction.
pub enum MultiRepoBookmarksTransactionResult {
    /// All bookmark updates succeeded.
    Success,
    /// One or more CAS operations failed. No bookmarks were moved.
    CasFailure,
}

impl MultiRepoBookmarksTransactionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

/// A transaction that atomically moves bookmarks across multiple repositories.
///
/// All repos MUST share the same MySQL shard (same write_connection).
/// The transaction accumulates bookmark operations across repos and commits
/// them all in a single SQL transaction.
pub struct MultiRepoBookmarksTransaction {
    ctx: CoreContext,
    write_connection: Connection,
    /// Track (repo_id, bookmark) pairs to prevent duplicates.
    seen: HashSet<(RepositoryId, BookmarkKey)>,
    ops: Vec<BookmarkOp>,
}

impl MultiRepoBookmarksTransaction {
    pub fn new(ctx: CoreContext, write_connection: Connection) -> Self {
        Self {
            ctx,
            write_connection,
            seen: HashSet::new(),
            ops: Vec::new(),
        }
    }

    /// Add an operation, ensuring each (repo_id, bookmark) pair is used at most once.
    fn push(&mut self, op: BookmarkOp) -> Result<()> {
        if !self.seen.insert((op.repo_id(), op.bookmark().clone())) {
            return Err(anyhow!(
                "({}, {}) bookmark was already used in this transaction",
                op.repo_id(),
                op.bookmark()
            ));
        }
        self.ops.push(op);
        Ok(())
    }

    pub fn update(
        &mut self,
        repo_id: RepositoryId,
        bookmark: &BookmarkKey,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.push(BookmarkOp::Update {
            repo_id,
            bookmark: bookmark.clone(),
            old_cs_id,
            new_cs_id,
            reason,
        })
    }

    pub fn create(
        &mut self,
        repo_id: RepositoryId,
        bookmark: &BookmarkKey,
        cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.push(BookmarkOp::Create {
            repo_id,
            bookmark: bookmark.clone(),
            cs_id,
            reason,
        })
    }

    pub fn delete(
        &mut self,
        repo_id: RepositoryId,
        bookmark: &BookmarkKey,
        old_cs_id: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.push(BookmarkOp::Delete {
            repo_id,
            bookmark: bookmark.clone(),
            old_cs_id,
            reason,
        })
    }

    /// Commit all ops atomically. Retries `RetryableError` up to
    /// `scm/mononoke:multi_repo_bookmark_max_retry_attempts`.
    pub async fn commit(self) -> Result<MultiRepoBookmarksTransactionResult> {
        if self.ops.is_empty() {
            return Ok(MultiRepoBookmarksTransactionResult::Success);
        }

        let max_attempts: u64 =
            justknobs::get_as::<u64>("scm/mononoke:multi_repo_bookmark_max_retry_attempts", None)?
                .max(1);

        let Self {
            ctx,
            write_connection,
            ops,
            ..
        } = self;

        retry_commit_loop(&ctx, max_attempts, || {
            attempt_commit(&ctx, &write_connection, &ops)
        })
        .await
    }
}

/// Run a single SQL transaction attempt for the multi-repo commit.
///
/// Returns `Ok(Success)` on success, `Err(LogicError)` if any CAS check
/// failed (entire transaction rolled back), `Err(RetryableError(_))` for
/// transient SQL errors that the caller should retry, and
/// `Err(Other(_))` for non-retryable infrastructure errors.
async fn attempt_commit(
    ctx: &CoreContext,
    write_connection: &Connection,
    ops: &[BookmarkOp],
) -> Result<MultiRepoBookmarksTransactionResult, BookmarkTransactionError> {
    let use_new_path = justknobs::eval("scm/mononoke:per_bookmark_locking", None, None)
        .context("Failed to read per_bookmark_locking JustKnob for multi-repo transaction")
        .map_err(BookmarkTransactionError::Other)?;

    let repo_ids: HashSet<_> = ops.iter().map(|op| op.repo_id()).collect();

    let txn = write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await
        .map_err(BookmarkTransactionError::Other)?;

    // Acquire locks and allocate IDs. Helper failures here are classified as
    // Other (not RetryableError) to preserve the pre-retry behavior — these
    // paths surface as fatal, identical to the original `?` propagation.
    let (mut txn, mut log) = if use_new_path {
        let txn = acquire_multi_repo_bookmark_locks(ops, txn)
            .await
            .map_err(BookmarkTransactionError::Other)?;
        let total_entries = ops.len();
        let (txn, ids) = allocate_multi_log_ids(txn, total_entries)
            .await
            .map_err(BookmarkTransactionError::Other)?;
        (txn, TransactionLog::from_pre_allocated(ids))
    } else {
        let (txn, next_log_ids) = find_next_log_ids(txn, &repo_ids)
            .await
            .map_err(BookmarkTransactionError::Other)?;
        (txn, TransactionLog::new(next_log_ids))
    };

    for op in ops {
        txn = op.execute(txn, &mut log).await?;
    }

    let txn = log
        .write(txn)
        .await
        .map_err(BookmarkTransactionError::RetryableError)?;
    txn.commit()
        .await
        .map_err(BookmarkTransactionError::Other)?;
    Ok(MultiRepoBookmarksTransactionResult::Success)
}

/// Execute `do_attempt` with retry-on-transient-error semantics.
///
/// Generic over the attempt closure so unit tests can drive the retry
/// machinery directly without needing a SQL fault-injection hook.
async fn retry_commit_loop<F, Fut>(
    ctx: &CoreContext,
    max_attempts: u64,
    mut do_attempt: F,
) -> Result<MultiRepoBookmarksTransactionResult>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<MultiRepoBookmarksTransactionResult, BookmarkTransactionError>>,
{
    let mut attempt = 0u64;
    loop {
        attempt += 1;
        match do_attempt().await {
            Ok(result) => {
                STATS::attempt_count.add_value(attempt as i64);
                STATS::success.add_value(1);
                return Ok(result);
            }
            Err(BookmarkTransactionError::LogicError) => {
                STATS::attempt_count.add_value(attempt as i64);
                STATS::cas_failure.add_value(1);
                return Ok(MultiRepoBookmarksTransactionResult::CasFailure);
            }
            Err(BookmarkTransactionError::RetryableError(err)) if attempt < max_attempts => {
                STATS::retry.add_value(1);
                ctx.scuba()
                    .clone()
                    .add("log_tag", "multi_repo_commit_retryable_error")
                    .add("attempt", attempt as i64)
                    .add("error", format!("{:#}", err))
                    .unsampled()
                    .log();
                continue;
            }
            Err(BookmarkTransactionError::RetryableError(err)) => {
                STATS::attempt_count.add_value(attempt as i64);
                STATS::retryable_error_exhausted.add_value(1);
                return Err(err.context(format!(
                    "Multi-repo bookmark transaction exhausted {} retry attempts",
                    max_attempts
                )));
            }
            Err(BookmarkTransactionError::Other(err)) => {
                STATS::attempt_count.add_value(attempt as i64);
                STATS::other_error.add_value(1);
                return Err(err);
            }
        }
    }
}

/// Find the next bookmark update log ID for each repo within the transaction.
async fn find_next_log_ids(
    mut txn: SqlTransaction,
    repo_ids: &HashSet<RepositoryId>,
) -> Result<(SqlTransaction, HashMap<RepositoryId, u64>)> {
    let mut next_ids = HashMap::new();
    for &repo_id in repo_ids {
        let (txn_, max_id_entries) =
            FindMaxBookmarkLogId::query_with_transaction(txn, &repo_id).await?;
        txn = txn_;
        let next_id = match &max_id_entries[..] {
            [(None,)] => 1,
            [(Some(max_existing),)] => *max_existing + 1,
            _ => {
                return Err(anyhow!(
                    "FindMaxBookmarkLogId returned multiple entries for repo {}: {:?}",
                    repo_id,
                    max_id_entries
                ));
            }
        };
        next_ids.insert(repo_id, next_id);
    }
    Ok((txn, next_ids))
}

/// Acquire per-bookmark locks for all operations, in sorted order to prevent deadlocks.
async fn acquire_multi_repo_bookmark_locks(
    ops: &[BookmarkOp],
    mut txn: SqlTransaction,
) -> Result<SqlTransaction> {
    // Collect and sort (repo_id, bookmark_name) pairs for deterministic ordering.
    // No dedup needed: MultiRepoBookmarksTransaction::push() rejects duplicate
    // (repo_id, bookmark) pairs via the `seen` HashSet.
    let mut lock_keys: Vec<(RepositoryId, &BookmarkName)> = ops
        .iter()
        .map(|op| (op.repo_id(), op.bookmark().name()))
        .collect();
    lock_keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));

    // Acquire locks in sorted order to prevent deadlocks: if two concurrent
    // transactions lock (repo1, bookmarkA) and (repo1, bookmarkB), both must
    // acquire them in the same order. Without this, T1 locking A-then-B and
    // T2 locking B-then-A would deadlock.
    for (repo_id, name) in lock_keys {
        let (txn_, rows) = AcquireBookmarkLock::query_with_transaction(txn, &repo_id, name).await?;
        txn = txn_;

        if rows.is_empty() {
            let data = [(&repo_id, name)];
            let (txn_, _) = EnsureBookmarkLockRow::query_with_transaction(txn, &data[..]).await?;
            let (txn_, _) =
                AcquireBookmarkLock::query_with_transaction(txn_, &repo_id, name).await?;
            txn = txn_;
        }
    }
    Ok(txn)
}

/// Allocate N log IDs from the global auto-increment sequence.
///
/// Allocate N log IDs from the global auto-increment sequence.
///
/// On first use (empty sequence table), seeds the table from the global
/// MAX(id) in bookmarks_update_log so that new IDs don't conflict with
/// existing log entries. Then inserts N rows and reads back the last
/// generated ID — consecutive single-row INSERTs produce consecutive
/// auto-increment IDs, so all N IDs can be derived from the last one.
async fn allocate_multi_log_ids(
    mut txn: SqlTransaction,
    count: usize,
) -> Result<(SqlTransaction, Vec<u64>)> {
    if count == 0 {
        return Ok((txn, vec![]));
    }

    // Seed the sequence table on first use so new IDs start above
    // existing entries in bookmarks_update_log.
    let (txn_, rows) = ReadMaxSequenceId::query_with_transaction(txn).await?;
    txn = txn_;
    if rows.first().and_then(|r| r.0).is_none() {
        let (txn_, global_max_rows) =
            FindGlobalMaxBookmarkLogId::query_with_transaction(txn).await?;
        txn = txn_;
        if let Some(max_id) = global_max_rows.first().and_then(|r| r.0) {
            let (txn_, _) = SeedSequenceId::query_with_transaction(txn, &max_id).await?;
            txn = txn_;
        }
    }

    // N individual INSERTs rather than a single multi-row INSERT because
    // MySQL's LAST_INSERT_ID() returns the FIRST generated value for
    // multi-row INSERTs, while SQLite's last_insert_rowid() returns the
    // LAST. Individual INSERTs give uniform behavior via ReadLastInsertId.
    // N is typically small (number of bookmarks in one transaction).
    for _ in 0..count {
        let (txn_, _) = AllocateBookmarkLogId::query_with_transaction(txn).await?;
        txn = txn_;
    }
    let (txn, rows) = ReadLastInsertId::query_with_transaction(txn).await?;
    let last_id = rows
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("ReadLastInsertId returned no rows"))?
        .0;
    anyhow::ensure!(
        last_id >= count as u64,
        "Auto-increment IDs inconsistent: last_id={} but expected at least {} IDs",
        last_id,
        count
    );
    let first_id = last_id - (count as u64) + 1;
    let ids: Vec<u64> = (first_id..=last_id).collect();
    Ok((txn, ids))
}

#[cfg(test)]
mod tests {
    use bookmarks::BookmarkKey;
    use bookmarks::BookmarkUpdateLog;
    use bookmarks::BookmarkUpdateLogId;
    use bookmarks::BookmarkUpdateReason;
    use bookmarks::Bookmarks;
    use bookmarks::Freshness;
    use context::CoreContext;
    use dbbookmarks::SqlBookmarksBuilder;
    use dbbookmarks::store::SqlBookmarks;
    use fbinit::FacebookInit;
    use futures::future::FutureExt;
    use futures::stream::TryStreamExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use sql_construct::SqlConstruct;

    use super::*;

    /// Test fixture providing two repos that share the same underlying DB.
    struct TwoRepoFixture {
        ctx: CoreContext,
        conn: Connection,
        repo_id_1: RepositoryId,
        repo_id_2: RepositoryId,
        bookmarks_1: SqlBookmarks,
        bookmarks_2: SqlBookmarks,
    }

    impl TwoRepoFixture {
        fn new(fb: FacebookInit) -> Result<Self> {
            let ctx = CoreContext::test_mock(fb);
            let repo_id_1 = RepositoryId::new(1);
            let repo_id_2 = RepositoryId::new(2);
            let builder = SqlBookmarksBuilder::with_sqlite_in_memory()?;
            let bookmarks_1 = builder.clone().with_repo_id(repo_id_1);
            let bookmarks_2 = builder.with_repo_id(repo_id_2);
            let conn = bookmarks_1.write_connection().clone();
            Ok(Self {
                ctx,
                conn,
                repo_id_1,
                repo_id_2,
                bookmarks_1,
                bookmarks_2,
            })
        }

        fn multi_txn(&self) -> MultiRepoBookmarksTransaction {
            MultiRepoBookmarksTransaction::new(self.ctx.clone(), self.conn.clone())
        }

        /// Create a bookmark in the given repo via a standard single-repo transaction.
        async fn set_bookmark(
            &self,
            bookmarks: &SqlBookmarks,
            key: &BookmarkKey,
            cs_id: ChangesetId,
        ) -> Result<()> {
            let mut txn = bookmarks.create_transaction(self.ctx.clone());
            txn.force_set(key, cs_id, BookmarkUpdateReason::TestMove)?;
            assert!(txn.commit().await.unwrap().is_some());
            Ok(())
        }

        /// Read a bookmark value from the given repo.
        async fn get_bookmark(
            &self,
            bookmarks: &SqlBookmarks,
            key: &BookmarkKey,
        ) -> Result<Option<ChangesetId>> {
            bookmarks
                .get(self.ctx.clone(), key, Freshness::MostRecent)
                .await
        }
    }

    #[mononoke::fbinit_test]
    async fn test_multi_repo_update_success(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
        f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.update(
            f.repo_id_2,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;

        let result = txn.commit().await?;
        assert!(result.is_success());

        assert_eq!(
            f.get_bookmark(&f.bookmarks_1, &bookmark).await?,
            Some(TWOS_CSID)
        );
        assert_eq!(
            f.get_bookmark(&f.bookmarks_2, &bookmark).await?,
            Some(TWOS_CSID)
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_cas_failure_rolls_back_all(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
        f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

        // R1: correct old value, R2: wrong old value (THREES instead of ONES)
        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.update(
            f.repo_id_2,
            &bookmark,
            TWOS_CSID,
            THREES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;

        let result = txn.commit().await?;
        assert!(!result.is_success());

        // Both should be unchanged
        assert_eq!(
            f.get_bookmark(&f.bookmarks_1, &bookmark).await?,
            Some(ONES_CSID)
        );
        assert_eq!(
            f.get_bookmark(&f.bookmarks_2, &bookmark).await?,
            Some(ONES_CSID)
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_rejects_duplicate_bookmark_in_same_repo(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;

        let result = txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        );
        assert!(result.is_err());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_same_bookmark_name_different_repos(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.update(
            f.repo_id_2,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        // Both accepted — different repo IDs
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_update_log_written_for_all_repos(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
        f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.update(
            f.repo_id_2,
            &bookmark,
            THREES_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        assert!(txn.commit().await?.is_success());

        // Each repo's log has 2 entries: the force_set then the multi-repo update.
        // Read all entries (id > 0) — works regardless of whether ids are
        // allocated per-repo (legacy) or globally (per_bookmark_locking), since
        // either way the only entries scoped to repo_id_X are these two.
        let log_1: Vec<_> = f
            .bookmarks_1
            .read_next_bookmark_log_entries(
                f.ctx.clone(),
                BookmarkUpdateLogId(0),
                10,
                Freshness::MostRecent,
            )
            .try_collect()
            .await?;
        assert_eq!(log_1.len(), 2);
        assert_eq!(log_1[1].from_changeset_id, Some(ONES_CSID));
        assert_eq!(log_1[1].to_changeset_id, Some(TWOS_CSID));
        assert_eq!(log_1[1].repo_id, f.repo_id_1);

        let log_2: Vec<_> = f
            .bookmarks_2
            .read_next_bookmark_log_entries(
                f.ctx.clone(),
                BookmarkUpdateLogId(0),
                10,
                Freshness::MostRecent,
            )
            .try_collect()
            .await?;
        assert_eq!(log_2.len(), 2);
        assert_eq!(log_2[1].from_changeset_id, Some(ONES_CSID));
        assert_eq!(log_2[1].to_changeset_id, Some(THREES_CSID));
        assert_eq!(log_2[1].repo_id, f.repo_id_2);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mixed_create_update_delete(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let master = BookmarkKey::new("master")?;
        let release = BookmarkKey::new("release")?;
        let feature = BookmarkKey::new("feature")?;

        f.set_bookmark(&f.bookmarks_1, &master, ONES_CSID).await?;
        f.set_bookmark(&f.bookmarks_2, &release, ONES_CSID).await?;

        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &master,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.create(
            f.repo_id_2,
            &feature,
            THREES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.delete(
            f.repo_id_2,
            &release,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        assert!(txn.commit().await?.is_success());

        assert_eq!(
            f.get_bookmark(&f.bookmarks_1, &master).await?,
            Some(TWOS_CSID)
        );
        assert_eq!(
            f.get_bookmark(&f.bookmarks_2, &feature).await?,
            Some(THREES_CSID)
        );
        assert_eq!(f.get_bookmark(&f.bookmarks_2, &release).await?, None);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_create_failure_rolls_back(fb: FacebookInit) -> Result<()> {
        let f = TwoRepoFixture::new(fb)?;
        let bookmark = BookmarkKey::new("master")?;

        f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
        f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

        // Update R1 + create R2 "master" (already exists) => should roll back both
        let mut txn = f.multi_txn();
        txn.update(
            f.repo_id_1,
            &bookmark,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.create(
            f.repo_id_2,
            &bookmark,
            THREES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;

        assert!(!txn.commit().await?.is_success());
        assert_eq!(
            f.get_bookmark(&f.bookmarks_1, &bookmark).await?,
            Some(ONES_CSID)
        );
        Ok(())
    }

    fn per_bookmark_locking_knobs() -> JustKnobsInMemory {
        JustKnobsInMemory::new(
            [(
                "scm/mononoke:per_bookmark_locking".to_string(),
                KnobVal::Bool(true),
            )]
            .into_iter()
            .collect(),
        )
    }

    #[mononoke::fbinit_test]
    async fn test_multi_repo_per_bookmark_locking(fb: FacebookInit) -> Result<()> {
        with_just_knobs_async(
            per_bookmark_locking_knobs(),
            async move {
                let f = TwoRepoFixture::new(fb)?;
                let bookmark = BookmarkKey::new("master")?;

                // Create bookmarks in both repos
                f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
                f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

                // Multi-repo update via new path
                let mut txn = f.multi_txn();
                txn.update(
                    f.repo_id_1,
                    &bookmark,
                    TWOS_CSID,
                    ONES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;
                txn.update(
                    f.repo_id_2,
                    &bookmark,
                    THREES_CSID,
                    ONES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;

                let result = txn.commit().await?;
                assert!(result.is_success());

                assert_eq!(
                    f.get_bookmark(&f.bookmarks_1, &bookmark).await?,
                    Some(TWOS_CSID)
                );
                assert_eq!(
                    f.get_bookmark(&f.bookmarks_2, &bookmark).await?,
                    Some(THREES_CSID)
                );

                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_multi_repo_per_bookmark_locking_cas_rollback(fb: FacebookInit) -> Result<()> {
        with_just_knobs_async(
            per_bookmark_locking_knobs(),
            async move {
                let f = TwoRepoFixture::new(fb)?;
                let bookmark = BookmarkKey::new("master")?;

                f.set_bookmark(&f.bookmarks_1, &bookmark, ONES_CSID).await?;
                f.set_bookmark(&f.bookmarks_2, &bookmark, ONES_CSID).await?;

                // R1 correct, R2 wrong old value
                let mut txn = f.multi_txn();
                txn.update(
                    f.repo_id_1,
                    &bookmark,
                    TWOS_CSID,
                    ONES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;
                txn.update(
                    f.repo_id_2,
                    &bookmark,
                    TWOS_CSID,
                    THREES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;

                let result = txn.commit().await?;
                assert!(!result.is_success());

                // Both should be unchanged (atomicity preserved)
                assert_eq!(
                    f.get_bookmark(&f.bookmarks_1, &bookmark).await?,
                    Some(ONES_CSID)
                );
                assert_eq!(
                    f.get_bookmark(&f.bookmarks_2, &bookmark).await?,
                    Some(ONES_CSID)
                );

                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_multi_repo_per_bookmark_locking_mixed_ops(fb: FacebookInit) -> Result<()> {
        with_just_knobs_async(
            per_bookmark_locking_knobs(),
            async move {
                let f = TwoRepoFixture::new(fb)?;
                let master = BookmarkKey::new("master")?;
                let release = BookmarkKey::new("release")?;
                let feature = BookmarkKey::new("feature")?;

                f.set_bookmark(&f.bookmarks_1, &master, ONES_CSID).await?;
                f.set_bookmark(&f.bookmarks_2, &release, ONES_CSID).await?;

                let mut txn = f.multi_txn();
                txn.update(
                    f.repo_id_1,
                    &master,
                    TWOS_CSID,
                    ONES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;
                txn.create(
                    f.repo_id_2,
                    &feature,
                    THREES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;
                txn.delete(
                    f.repo_id_2,
                    &release,
                    ONES_CSID,
                    BookmarkUpdateReason::TestMove,
                )?;
                assert!(txn.commit().await?.is_success());

                assert_eq!(
                    f.get_bookmark(&f.bookmarks_1, &master).await?,
                    Some(TWOS_CSID)
                );
                assert_eq!(
                    f.get_bookmark(&f.bookmarks_2, &feature).await?,
                    Some(THREES_CSID)
                );
                assert_eq!(f.get_bookmark(&f.bookmarks_2, &release).await?, None);
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_retry_on_retryable_error(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let attempts = std::cell::Cell::new(0u64);

        let result = retry_commit_loop(&ctx, 5, || {
            attempts.set(attempts.get() + 1);
            let attempt = attempts.get();
            async move {
                if attempt == 1 {
                    Err(BookmarkTransactionError::RetryableError(anyhow!(
                        "simulated transient failure"
                    )))
                } else {
                    Ok(MultiRepoBookmarksTransactionResult::Success)
                }
            }
        })
        .await?;

        assert!(result.is_success());
        assert_eq!(attempts.get(), 2, "should succeed on second attempt");
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_retryable_error_exhausted(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let attempts = std::cell::Cell::new(0u64);

        let result = retry_commit_loop(&ctx, 3, || {
            attempts.set(attempts.get() + 1);
            async move {
                Err::<MultiRepoBookmarksTransactionResult, _>(
                    BookmarkTransactionError::RetryableError(anyhow!(
                        "simulated permanent transient failure"
                    )),
                )
            }
        })
        .await;

        let err = match result {
            Ok(_) => panic!("expected Err after exhausting retries"),
            Err(e) => e,
        };
        assert_eq!(
            attempts.get(),
            3,
            "should attempt exactly max_attempts times"
        );
        let err_msg = format!("{:#}", err);
        assert!(
            err_msg.contains("exhausted 3 retry attempts"),
            "error should describe exhaustion: {}",
            err_msg
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_other_error_not_retried(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let attempts = std::cell::Cell::new(0u64);

        let result = retry_commit_loop(&ctx, 5, || {
            attempts.set(attempts.get() + 1);
            async move {
                Err::<MultiRepoBookmarksTransactionResult, _>(BookmarkTransactionError::Other(
                    anyhow!("non-retryable infrastructure failure"),
                ))
            }
        })
        .await;

        assert!(result.is_err(), "Other error must propagate");
        assert_eq!(attempts.get(), 1, "Other error must not retry");
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_logic_error_translates_to_cas_failure(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let attempts = std::cell::Cell::new(0u64);

        let result = retry_commit_loop(&ctx, 5, || {
            attempts.set(attempts.get() + 1);
            async move {
                Err::<MultiRepoBookmarksTransactionResult, _>(BookmarkTransactionError::LogicError)
            }
        })
        .await?;

        assert!(!result.is_success(), "LogicError should map to CasFailure");
        assert_eq!(attempts.get(), 1, "LogicError must not retry");
        Ok(())
    }
}
