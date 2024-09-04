/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use bookmarks::BookmarkTransaction;
use bookmarks::BookmarkTransactionHook;
use bookmarks_movement::BookmarkInfoData;
use bookmarks_movement::BookmarkInfoTransaction;
use bookmarks_movement::TransactionWithHooks;
use bytes::Bytes;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use mononoke_api::BookmarkKey;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use slog::info;

/// Enum determining the nature of error reporting for bookmark operations
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum BookmarkOperationErrorReporting {
    /// Report the errors occuring during bookmark operation as-is
    Plain,
    /// Report the errors occuring during bookmark operation with added context
    /// highlighting which specific operation failed
    WithContext,
}

/// Struct representing a bookmark operation.
pub struct BookmarkOperation {
    pub bookmark_key: BookmarkKey,
    pub operation_type: BookmarkOperationType,
}

impl BookmarkOperation {
    pub fn new(
        bookmark_key: BookmarkKey,
        old_changeset: Option<ChangesetId>,
        new_changeset: Option<ChangesetId>,
    ) -> anyhow::Result<Self> {
        let operation_type = BookmarkOperationType::from_changesets(old_changeset, new_changeset)?;
        Ok(Self {
            bookmark_key,
            operation_type,
        })
    }

    pub fn is_delete(&self) -> bool {
        self.operation_type.is_delete()
    }
}

/// Enum representing the type of bookmark operation.
pub enum BookmarkOperationType {
    /// Operation for creating the bookmark at changeset id
    Create(ChangesetId),
    /// Operation for moving the bookmark from old_changeset to new_changeset
    Move(ChangesetId, ChangesetId),
    /// Operation for deleting the bookmark at changeset id
    Delete(ChangesetId),
}

impl BookmarkOperationType {
    pub fn from_changesets(
        old_changeset: Option<ChangesetId>,
        new_changeset: Option<ChangesetId>,
    ) -> anyhow::Result<Self> {
        let op = match (old_changeset, new_changeset) {
            // The bookmark already exists. Instead of creating it, we need to move it.
            (Some(old), Some(new)) => Self::Move(old, new),
            // The bookmark doesn't yet exist. Create it.
            (None, Some(new)) => Self::Create(new),
            // The bookmark exists, but we're deleting it.
            (Some(old), None) => Self::Delete(old),
            _ => anyhow::bail!(
                "Invalid bookmark operation. Both old and new changesets cannot be None"
            ),
        };
        Ok(op)
    }

    pub fn is_delete(&self) -> bool {
        match self {
            Self::Delete(_) => true,
            _ => false,
        }
    }
}

/// Method responsible for either creating, moving or deleting a bookmark in gitimport and gitserver.
pub async fn set_bookmark<R: MononokeRepo>(
    ctx: &CoreContext,
    repo_context: &RepoContext<R>,
    bookmark_operation: &BookmarkOperation,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    error_reporting: BookmarkOperationErrorReporting,
) -> Result<(), MononokeError> {
    let bookmark_key = &bookmark_operation.bookmark_key;
    let name = bookmark_key.name();
    match bookmark_operation.operation_type {
        BookmarkOperationType::Create(new_changeset) => {
            let op_result = repo_context
                .create_bookmark(bookmark_key, new_changeset, pushvars)
                .await;
            if error_reporting == BookmarkOperationErrorReporting::WithContext {
                op_result.with_context(|| format!("failed to create bookmark {name}"))?;
            } else {
                op_result?;
            }
            info!(
                ctx.logger(),
                "Bookmark: \"{name}\": {new_changeset:?} (created)"
            )
        }
        BookmarkOperationType::Move(old_changeset, new_changeset) => {
            if old_changeset != new_changeset {
                let op_result = repo_context
                    .move_bookmark(
                        bookmark_key,
                        new_changeset,
                        Some(old_changeset),
                        allow_non_fast_forward,
                        pushvars,
                    )
                    .await;
                if error_reporting == BookmarkOperationErrorReporting::WithContext {
                    op_result.with_context(|| {
                        format!(
                            "failed to move bookmark {name} from {old_changeset:?} to {new_changeset:?}"
                        )
                    })?;
                } else {
                    op_result?;
                }
                info!(
                    ctx.logger(),
                    "Bookmark: \"{name}\": {new_changeset:?} (moved from {old_changeset:?})"
                );
            } else {
                info!(
                    ctx.logger(),
                    "Bookmark: \"{name}\": {new_changeset:?} (already up-to-date)"
                );
            }
        }
        BookmarkOperationType::Delete(old_changeset) => {
            let op_result = repo_context
                .delete_bookmark(bookmark_key, Some(old_changeset), pushvars)
                .await;
            if error_reporting == BookmarkOperationErrorReporting::WithContext {
                op_result.with_context(|| format!("failed to delete bookmark {name}"))?;
            } else {
                op_result?;
            }
            info!(
                ctx.logger(),
                "Bookmark: \"{name}\": {old_changeset:?} (deleted)"
            );
        }
    }
    Result::Ok(())
}

/// Method responsible for multiple bookmark moves, where each bookmark move can either be creating,
/// moving or deleting a bookmark in gitimport and gitserver.
pub async fn set_bookmarks<R: MononokeRepo>(
    ctx: &CoreContext,
    repo_context: &RepoContext<R>,
    bookmark_operations: Vec<BookmarkOperation>,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    error_reporting: BookmarkOperationErrorReporting,
) -> Result<(), MononokeError> {
    let mut bookmark_transaction = None;
    let mut transaction_hooks = vec![];
    let mut bookmark_infos = vec![];
    for bookmark_operation in bookmark_operations {
        let move_bookmark_result = move_bookmark(
            repo_context,
            bookmark_operation,
            pushvars,
            allow_non_fast_forward,
            bookmark_transaction,
            transaction_hooks,
            error_reporting,
        )
        .await?;
        bookmark_transaction = move_bookmark_result.bookmark_transaction;
        transaction_hooks = move_bookmark_result.transaction_hooks;
        bookmark_infos.push(move_bookmark_result.info_data);
    }
    // All bookmarks are covered, finally commit the transaction.
    let bookmark_transaction =
        bookmark_transaction.ok_or_else(|| anyhow::anyhow!("No bookmark transaction found"))?;
    let transaction = TransactionWithHooks::new(bookmark_transaction, transaction_hooks);
    transaction
        .commit()
        .await
        .context("Failed to commit bookmark move transaction")?;
    // For all the bookmarks that were moved, write the required data to the appropriate logger
    stream::iter(bookmark_infos)
        .for_each(|bookmark_info| {
            let repo = repo_context.repo_arc();
            let ctx = ctx.clone();
            async move { bookmark_info.log(&ctx, &repo).await }
        })
        .await;
    Result::Ok(())
}

async fn move_bookmark<R: MononokeRepo>(
    repo_context: &RepoContext<R>,
    bookmark_operation: BookmarkOperation,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    bookmark_transaction: Option<Box<dyn BookmarkTransaction>>,
    transaction_hooks: Vec<BookmarkTransactionHook>,
    error_reporting: BookmarkOperationErrorReporting,
) -> Result<MoveBookmarkResult, MononokeError> {
    let bookmark_key = &bookmark_operation.bookmark_key;
    let name = bookmark_key.name();
    let bookmark_info_transaction = match bookmark_operation.operation_type {
        BookmarkOperationType::Create(new_changeset) => {
            let op_result = repo_context
                .create_bookmark_with_transaction(
                    bookmark_key,
                    new_changeset,
                    pushvars,
                    bookmark_transaction,
                    transaction_hooks,
                )
                .await;
            if error_reporting == BookmarkOperationErrorReporting::WithContext {
                op_result.with_context(|| format!("failed to create bookmark {name}"))?
            } else {
                op_result?
            }
        }
        BookmarkOperationType::Move(old_changeset, new_changeset) => {
            if old_changeset != new_changeset {
                let op_result = repo_context
                    .move_bookmark_with_transaction(
                        bookmark_key,
                        new_changeset,
                        Some(old_changeset),
                        allow_non_fast_forward,
                        pushvars,
                        bookmark_transaction,
                        transaction_hooks,
                    )
                    .await;
                if error_reporting == BookmarkOperationErrorReporting::WithContext {
                    op_result.with_context(|| {
                        format!(
                            "failed to move bookmark {name} from {old_changeset:?} to {new_changeset:?}"
                        )
                    })?
                } else {
                    op_result?
                }
            } else {
                Err(MononokeError::InvalidRequest(
                    "Bookmark: \"{name}\" already points to commit {new_changeset:?}".to_string(),
                ))?
            }
        }
        BookmarkOperationType::Delete(old_changeset) => {
            let op_result = repo_context
                .delete_bookmark_with_transaction(
                    bookmark_key,
                    Some(old_changeset),
                    pushvars,
                    bookmark_transaction,
                )
                .await;
            if error_reporting == BookmarkOperationErrorReporting::WithContext {
                op_result.with_context(|| format!("failed to delete bookmark {name}"))?
            } else {
                op_result?
            }
        }
    };
    let BookmarkInfoTransaction {
        info_data,
        transaction,
    } = bookmark_info_transaction;
    let TransactionWithHooks {
        transaction,
        txn_hooks,
    } = transaction;
    Result::Ok(MoveBookmarkResult {
        info_data,
        bookmark_transaction: Some(transaction),
        transaction_hooks: txn_hooks,
    })
}

struct MoveBookmarkResult {
    info_data: BookmarkInfoData,
    bookmark_transaction: Option<Box<dyn BookmarkTransaction>>,
    transaction_hooks: Vec<BookmarkTransactionHook>,
}
