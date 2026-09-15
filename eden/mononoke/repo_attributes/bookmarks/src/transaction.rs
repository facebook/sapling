/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use context::CoreContext;
use futures::future::BoxFuture;
use mononoke_types::ChangesetId;
use sql_ext::Transaction;
use thiserror::Error;

use crate::log::BookmarkUpdateReason;

#[derive(Debug, Error)]
pub enum BookmarkTransactionError {
    // The transaction modifying bookmarks tables should be retried
    #[error("BookmarkTransactionError::RetryableError")]
    RetryableError(#[source] Error),
    // Transacton was rolled back, we consider this a logic error,
    // which may prompt retry higher in the stack. This can happen
    // for example if some other bookmark update won the race and
    // the entire pushrebase needs to be retried
    #[error("BookmarkTransactionError::LogicError")]
    LogicError,
    // A modern_sync mirror move that the replica already applied (a lost-ack
    // replay), identified because the replica's current log id for the bookmark
    // is already at or past the batch's source log ids, so no move is left to
    // apply. The CAS matching no rows alone can mean any divergence; the
    // already-advanced log id is what marks the replay. Not an error to retry:
    // reusing the source log id makes the move idempotent and safe to replay.
    #[error("BookmarkTransactionError::AlreadyProcessed")]
    AlreadyProcessed,
    // Something unexpected went wrong
    #[error("BookmarkTransactionError::Other")]
    Other(#[from] Error),
}

/// Marker error for a modern_sync mirror bookmark move that the replica
/// already applied (a lost-ack replay). `commit`/`commit_with_hooks` return it
/// inside their `anyhow::Error` so higher layers can downcast for it and map
/// the condition to a distinct response code instead of a generic failure.
#[derive(Debug, Error)]
#[error("bookmark move already processed")]
pub struct BookmarkMoveAlreadyProcessed;

pub type BookmarkTransactionHook = Arc<
    dyn Fn(
            CoreContext,
            Transaction,
        ) -> BoxFuture<'static, Result<Transaction, BookmarkTransactionError>>
        + Sync
        + Send,
>;

/// One bookmark move in a modern_sync mirror batch (see `mirror_batch`).
/// Carries the source repo's bookmarks_update_log entry id, the move's old and
/// new changesets, and its reason, so the `*_shadow` replica's log row matches
/// the source row for row. modern_sync mirrors only the main publishing
/// bookmark, which is never deleted, so `new` is always set. `old` is `None`
/// only for the first move of a brand-new repo, when the bookmark is created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorBookmarkMove {
    pub log_id: u64,
    pub old: Option<ChangesetId>,
    pub new: ChangesetId,
    pub reason: BookmarkUpdateReason,
}

pub trait BookmarkTransaction: Send + Sync + 'static {
    /// Adds set() operation to the transaction set.
    /// Updates a bookmark's value. Bookmark should already exist and point to `old_cs`, otherwise
    /// committing the transaction will fail. The Bookmark should also not be Scratch.
    fn update(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds a modern_sync batch mirror operation to the transaction set.
    /// Applies a contiguous chain of source bookmark moves to a `*_shadow`
    /// replica as one compare-and-swap, then writes one bookmarks_update_log row
    /// per move. Each row reuses the source move's log id, changesets, and
    /// reason, so the replica's log matches the source row for row. The store
    /// applies only the moves the replica has not seen yet, so a replayed
    /// (lost-ack) batch is idempotent even when a retry regroups the moves.
    ///
    /// If the first unseen move's `old` is `None`, the store creates the
    /// bookmark with `kind` instead of moving it; `kind` is ignored otherwise.
    ///
    /// `moves` must be non-empty, ordered by strictly increasing `log_id`, and
    /// contiguous (`moves[i].old` equals `moves[i-1].new`). Only modern_sync
    /// calls it; every other caller uses `update`.
    fn mirror_batch(
        &mut self,
        bookmark: &BookmarkKey,
        kind: BookmarkKind,
        moves: Vec<MirrorBookmarkMove>,
    ) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Creates a bookmark. BookmarkKey should not already exist, otherwise committing the
    /// transaction will fail. The resulting Bookmark will be PullDefault.
    fn create(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Forces creation of a bookmark. If bookmark already exists, it will be overwritten.
    /// This should only be used in mirror upload operations.
    fn creates_or_updates(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_set() operation to the transaction set.
    /// Unconditionally sets the new value of the bookmark. Succeeds regardless of whether bookmark
    /// exists or not.
    fn force_set(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete(
        &mut self,
        bookmark: &BookmarkKey,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_delete operation to the transaction set.
    /// Deletes bookmark unconditionally.
    fn force_delete(&mut self, bookmark: &BookmarkKey, reason: BookmarkUpdateReason) -> Result<()>;

    /// Adds a scratch bookmark update operation to the transaction set.
    /// Updates the changeset referenced by the bookmark, if it is already a scratch bookmark.
    fn update_scratch(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()>;

    /// Adds a scratch bookmark create operation to the transaction set.
    /// Creates a new bookmark, configured as scratch. It should not exist already.
    fn create_scratch(&mut self, bookmark: &BookmarkKey, new_cs: ChangesetId) -> Result<()>;

    /// Adds a scratch bookmark delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete_scratch(&mut self, bookmark: &BookmarkKey, old_cs: ChangesetId) -> Result<()>;

    /// Adds a publishing bookmark create operation to the transaction set.
    /// Creates a new bookmark, configured as publishing. It should not exist already.
    fn create_publishing(
        &mut self,
        bookmark: &BookmarkKey,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Commits the transaction. Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Logical failure is indicated by
    /// returning a successful `None` value; infrastructure failure is reported via an Error.
    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<Option<u64>>>;

    /// Commits the bookmarks update along with any changes injected by all the BookmarkTransactionHooks. The
    /// future returns Some(log_id) if the bookmarks has moved, and None otherwise. Infrastructure errors
    /// are reported via the Error.
    fn commit_with_hooks(
        self: Box<Self>,
        txn_hooks: Vec<BookmarkTransactionHook>,
    ) -> BoxFuture<'static, Result<Option<u64>>>;
}
