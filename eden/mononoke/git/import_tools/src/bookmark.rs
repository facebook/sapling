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
use bytes::Bytes;
use context::CoreContext;
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
