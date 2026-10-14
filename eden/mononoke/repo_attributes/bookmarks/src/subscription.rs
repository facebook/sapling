/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::BookmarkUpdateLogId;

#[async_trait]
pub trait BookmarksSubscription: Send + Sync + 'static {
    /// Refresh this subscription with new updated bookmarks
    async fn refresh(&mut self, ctx: &CoreContext) -> Result<()>;

    /// Get current bookmarks and the update-log IDs that produced their values.
    ///
    /// The log ID is `None` only for legacy or directly inserted rows whose current value has no
    /// corresponding bookmark update-log entry.
    fn bookmarks(
        &self,
    ) -> &HashMap<BookmarkKey, (ChangesetId, BookmarkKind, Option<BookmarkUpdateLogId>)>;
}
