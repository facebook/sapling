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
use metaconfig_types::BookmarkAttrs;
use mononoke_types::ChangesetId;

use crate::errors::MononokeError;
use crate::permissions::WritePermissionsModel;
use crate::repo_write::RepoWriteContext;

impl RepoWriteContext {
    /// Delete a bookmark.
    pub async fn delete_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        old_target: Option<ChangesetId>,
        pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<(), MononokeError> {
        let bookmark = bookmark.as_ref();
        self.check_method_permitted("delete_bookmark")?;

        let bookmark = BookmarkName::new(bookmark)?;
        let bookmark_attrs =
            BookmarkAttrs::new(self.ctx().fb, self.config().bookmarks.clone()).await?;

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
        let mut op = bookmarks_movement::DeleteBookmarkOp::new(
            &bookmark,
            old_target,
            BookmarkUpdateReason::ApiRequest,
        )
        .with_pushvars(pushvars);

        if let WritePermissionsModel::ServiceIdentity(service_identity) = &self.permissions_model {
            op = op.for_service(service_identity, &self.config().source_control_service);
        }

        op.run(
            self.ctx(),
            self.inner_repo(),
            &self.config().infinitepush,
            &bookmark_attrs,
            self.readonly_fetcher(),
        )
        .await?;

        Ok(())
    }
}
