/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
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
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use slog::info;

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
    ) -> Result<Self> {
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
    ) -> Result<Self> {
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
pub async fn set_bookmark(
    ctx: &CoreContext,
    repo_context: &RepoContext,
    bookmark_operation: &BookmarkOperation,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    affected_changesets_limit: Option<usize>,
) -> Result<()> {
    let bookmark_key = &bookmark_operation.bookmark_key;
    let name = bookmark_key.name();
    match bookmark_operation.operation_type {
        BookmarkOperationType::Create(new_changeset) => {
            repo_context
                .create_bookmark(
                    bookmark_key,
                    new_changeset,
                    pushvars,
                    affected_changesets_limit,
                )
                .await
                .with_context(|| format!("failed to create bookmark {name}"))?;
            info!(
                ctx.logger(),
                "Bookmark: \"{name}\": {new_changeset:?} (created)"
            )
        }
        BookmarkOperationType::Move(old_changeset, new_changeset) => {
            if old_changeset != new_changeset {
                repo_context
                    .move_bookmark(
                        bookmark_key,
                        new_changeset,
                        Some(old_changeset),
                        allow_non_fast_forward,
                        pushvars,
                        affected_changesets_limit,
                    )
                    .await
                    .with_context(|| format!("failed to move bookmark {name} from {old_changeset:?} to {new_changeset:?}"))?;
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
            repo_context
                .delete_bookmark(bookmark_key, Some(old_changeset), pushvars)
                .await
                .with_context(|| format!("failed to delete bookmark {name}"))?;
            info!(
                ctx.logger(),
                "Bookmark: \"{name}\": {old_changeset:?} (deleted)"
            );
        }
    }
    Ok(())
}

/// Method responsible for multiple bookmark moves, where each bookmark move can either be creating,
/// moving or deleting a bookmark in gitimport and gitserver.
pub async fn set_bookmarks(
    ctx: &CoreContext,
    repo_context: &RepoContext,
    bookmark_operations: Vec<BookmarkOperation>,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    affected_changesets_limit: Option<usize>,
) -> Result<()> {
    let mut bookmark_transaction = None;
    let mut transaction_hooks = vec![];
    let mut bookmark_infos = vec![];
    for bookmark_operation in bookmark_operations {
        let move_bookmark_result = move_bookmark(
            repo_context,
            bookmark_operation,
            pushvars,
            allow_non_fast_forward,
            affected_changesets_limit,
            bookmark_transaction,
            transaction_hooks,
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
    Ok(())
}

async fn move_bookmark(
    repo_context: &RepoContext,
    bookmark_operation: BookmarkOperation,
    pushvars: Option<&HashMap<String, Bytes>>,
    allow_non_fast_forward: bool,
    affected_changesets_limit: Option<usize>,
    bookmark_transaction: Option<Box<dyn BookmarkTransaction>>,
    transaction_hooks: Vec<BookmarkTransactionHook>,
) -> Result<MoveBookmarkResult> {
    let bookmark_key = &bookmark_operation.bookmark_key;
    let name = bookmark_key.name();
    let bookmark_info_transaction = match bookmark_operation.operation_type {
        BookmarkOperationType::Create(new_changeset) => repo_context
            .create_bookmark_with_transaction(
                bookmark_key,
                new_changeset,
                pushvars,
                affected_changesets_limit,
                bookmark_transaction,
                transaction_hooks,
            )
            .await
            .with_context(|| format!("failed to create bookmark {name}"))?,
        BookmarkOperationType::Move(old_changeset, new_changeset) => {
            if old_changeset != new_changeset {
                repo_context
                    .move_bookmark_with_transaction(
                        bookmark_key,
                        new_changeset,
                        Some(old_changeset),
                        allow_non_fast_forward,
                        pushvars,
                        affected_changesets_limit,
                        bookmark_transaction,
                        transaction_hooks,
                    )
                    .await
                    .with_context(|| format!("failed to move bookmark {name} from {old_changeset:?} to {new_changeset:?}"))?
            } else {
                anyhow::bail!("Bookmark: \"{name}\" already points to commit {new_changeset:?}");
            }
        }
        BookmarkOperationType::Delete(old_changeset) => repo_context
            .delete_bookmark_with_transaction(
                bookmark_key,
                Some(old_changeset),
                pushvars,
                bookmark_transaction,
            )
            .await
            .with_context(|| format!("failed to delete bookmark {name}"))?,
    };
    let BookmarkInfoTransaction {
        info_data,
        transaction,
    } = bookmark_info_transaction;
    let TransactionWithHooks {
        transaction,
        txn_hooks,
    } = transaction;
    Ok(MoveBookmarkResult {
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
