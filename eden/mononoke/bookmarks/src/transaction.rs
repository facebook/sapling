/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use futures::future::BoxFuture;
use mononoke_types::ChangesetId;
use sql::Transaction;
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
    // Something unexpected went wrong
    #[error("BookmarkTransactionError::Other")]
    Other(#[from] Error),
}

pub type BookmarkTransactionHook = Arc<
    dyn Fn(
            CoreContext,
            Transaction,
        ) -> BoxFuture<'static, Result<Transaction, BookmarkTransactionError>>
        + Sync
        + Send,
>;

pub trait BookmarkTransaction: Send + Sync + 'static {
    /// Adds set() operation to the transaction set.
    /// Updates a bookmark's value. Bookmark should already exist and point to `old_cs`, otherwise
    /// committing the transaction will fail. The Bookmark should also not be Scratch.
    fn update(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Creates a bookmark. BookmarkName should not already exist, otherwise committing the
    /// transaction will fail. The resulting Bookmark will be PullDefault.
    fn create(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_set() operation to the transaction set.
    /// Unconditionally sets the new value of the bookmark. Succeeds regardless of whether bookmark
    /// exists or not.
    fn force_set(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete(
        &mut self,
        bookmark: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_delete operation to the transaction set.
    /// Deletes bookmark unconditionally.
    fn force_delete(&mut self, bookmark: &BookmarkName, reason: BookmarkUpdateReason)
    -> Result<()>;

    /// Adds a scratch bookmark update operation to the transaction set.
    /// Updates the changeset referenced by the bookmark, if it is already a scratch bookmark.
    fn update_scratch(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()>;

    /// Adds a scratch bookmark create operation to the transaction set.
    /// Creates a new bookmark, configured as scratch. It should not exist already.
    fn create_scratch(&mut self, bookmark: &BookmarkName, new_cs: ChangesetId) -> Result<()>;

    /// Adds a scratch bookmark delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete_scratch(&mut self, bookmark: &BookmarkName, old_cs: ChangesetId) -> Result<()>;

    /// Adds a publishing bookmark create operation to the transaction set.
    /// Creates a new bookmark, configured as publishing. It should not exist already.
    fn create_publishing(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Commits the transaction. Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Logical failure is indicated by
    /// returning a successful `false` value; infrastructure failure is reported via an Error.
    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<bool>>;

    /// Commits the bookmarks update along with any changes injected by the BookmarkTransactionHook. The
    /// future returns true if the bookmarks has moved, and false otherwise. Infrastructure errors
    /// are reported via the Error.
    fn commit_with_hook(
        self: Box<Self>,
        txn_hook: BookmarkTransactionHook,
    ) -> BoxFuture<'static, Result<bool>>;
}
