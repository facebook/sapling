/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bytes::Bytes;
use mononoke_types::ChangesetId;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

impl RepoContext {
    /// Delete a bookmark.
    pub async fn delete_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        old_target: Option<ChangesetId>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        self.start_write()?;

        let bookmark = bookmark.as_ref();
        let bookmark = BookmarkName::new(bookmark)?;

        // We need to find out where the bookmark currently points to in order
        // to delete it.  Make sure to bypass any out-of-date caches.
        let old_target = match old_target {
            Some(old_target) => old_target,
            None => self
                .blob_repo()
                .bookmarks()
                .get(self.ctx().clone(), &bookmark)
                .await
                .context("Failed to fetch old bookmark target")?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("bookmark '{}' does not exist", bookmark))
                })?,
        };

        // Delete the bookmark.
        let op = bookmarks_movement::DeleteBookmarkOp::new(
            &bookmark,
            old_target,
            BookmarkUpdateReason::ApiRequest,
        )
        .with_pushvars(pushvars);

        op.run(
            self.ctx(),
            self.authorization_context(),
            self.inner_repo(),
            &self.config().infinitepush,
        )
        .await?;

        Ok(())
    }
}
